//! Opt-in OANDA MT5 authentication for the current Windows user.

mod credentials;
mod dialog;

use crate::core::models::UiSnapshot;
use crate::core::ports::SnapshotExchangePort;
use crate::core::types::{BrokerId, MonoNs};
use crate::runtime::TerminalManager;
use credentials::Credential;
use parking_lot::Mutex;
use std::cell::Cell;
use std::os::windows::ffi::OsStringExt;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::path::{Path, PathBuf};
use std::ptr::{null, null_mut};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use windows_sys::Win32::Foundation::{HWND, WAIT_OBJECT_0, WAIT_TIMEOUT};
use windows_sys::Win32::System::Threading::{
    CreateEventW, OpenProcess, QueryFullProcessImageNameW, SetEvent, WaitForMultipleObjects,
    INFINITE, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE,
};
use windows_sys::Win32::UI::Accessibility::{SetWinEventHook, UnhookWinEvent, HWINEVENTHOOK};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, IsWindowVisible, MsgWaitForMultipleObjectsEx, PeekMessageW, TranslateMessage,
    CHILDID_SELF, EVENT_OBJECT_HIDE, EVENT_OBJECT_SHOW, MWMO_INPUTAVAILABLE, OBJID_WINDOW,
    PM_REMOVE, QS_ALLINPUT, WINEVENT_OUTOFCONTEXT,
};

#[derive(Clone, Copy)]
enum Status {
    NotRegistered,
    WaitingForMt5,
    WaitingForOtp,
    Sent { live_tick_before: Option<MonoNs> },
    Message(&'static str),
}

pub(crate) struct OandaAutoLogin {
    broker_id: BrokerId,
    registered: bool,
    owner: isize,
    watched_pid: Option<u32>,
    watcher: Option<Watcher>,
    status: Arc<Mutex<Status>>,
}

impl OandaAutoLogin {
    pub(crate) fn new(broker_id: BrokerId) -> Self {
        let (registered, status) = match Credential::load() {
            Ok(Some(_)) => (true, Status::WaitingForMt5),
            Ok(None) => (false, Status::NotRegistered),
            Err(reason) => (false, Status::Message(reason)),
        };
        Self {
            broker_id,
            registered,
            owner: 0,
            watched_pid: None,
            watcher: None,
            status: Arc::new(Mutex::new(status)),
        }
    }

    pub(crate) fn set_owner(&mut self, hwnd: isize) {
        self.owner = hwnd;
    }

    pub(crate) fn is_registered(&self) -> bool {
        self.registered
    }

    pub(crate) fn register(&mut self) {
        match credentials::prompt_and_save(self.owner as HWND) {
            Ok(true) => {
                self.watcher = None;
                self.watched_pid = None;
                self.registered = true;
                self.status = Arc::new(Mutex::new(Status::WaitingForMt5));
            }
            Ok(false) => {}
            Err(reason) => *self.status.lock() = Status::Message(reason),
        }
    }

    pub(crate) fn remove(&mut self) {
        match credentials::remove() {
            Ok(()) => {
                self.watcher = None;
                self.watched_pid = None;
                self.registered = false;
                self.status = Arc::new(Mutex::new(Status::NotRegistered));
            }
            Err(reason) => *self.status.lock() = Status::Message(reason),
        }
    }

    pub(crate) fn sync(
        &mut self,
        terminals: &TerminalManager,
        exchange: &Arc<dyn SnapshotExchangePort>,
    ) {
        if !self.registered {
            return;
        }
        let pid = terminals.get_status(self.broker_id).pid();
        if pid == self.watched_pid {
            return;
        }
        self.watcher = None;
        self.watched_pid = pid;
        self.status = Arc::new(Mutex::new(Status::WaitingForMt5));
        let (Some(pid), Some(exe)) = (pid, terminals.get_exe_path(self.broker_id)) else {
            return;
        };
        let target = Target {
            pid,
            exe: exe.clone(),
            broker_id: self.broker_id,
            exchange: Arc::clone(exchange),
            status: Arc::clone(&self.status),
        };
        *self.status.lock() = Status::WaitingForOtp;
        self.watcher = Watcher::start(target);
        if self.watcher.is_none() {
            *self.status.lock() = Status::Message("OANDA MT5の監視を開始できませんでした。");
        }
    }

    pub(crate) fn status_text(&self, snapshot: &UiSnapshot) -> &'static str {
        match *self.status.lock() {
            Status::NotRegistered => "未登録",
            Status::WaitingForMt5 => "登録済み・OANDA MT5の起動待ち",
            Status::WaitingForOtp => "登録済み・OTP画面の表示待ち",
            Status::Sent { live_tick_before }
                if live_tick(snapshot, self.broker_id) > live_tick_before =>
            {
                "OTP送信後のOANDAの価格受信を確認しました。"
            }
            Status::Sent { .. } => "OTP送信済み・OANDAの価格受信待ち",
            Status::Message(message) => message,
        }
    }
}

fn live_tick(snapshot: &UiSnapshot, broker_id: BrokerId) -> Option<MonoNs> {
    snapshot
        .broker_overviews
        .iter()
        .find(|b| b.broker_id == broker_id)?
        .health
        .last_live_tick_rx_mono
}

struct Target {
    pid: u32,
    exe: PathBuf,
    broker_id: BrokerId,
    exchange: Arc<dyn SnapshotExchangePort>,
    status: Arc<Mutex<Status>>,
}

struct Watcher {
    stop: Arc<OwnedHandle>,
}

impl Watcher {
    fn start(target: Target) -> Option<Self> {
        let stop = unsafe { CreateEventW(null(), 1, 0, null()) };
        if stop.is_null() {
            return None;
        }
        let stop = Arc::new(unsafe { OwnedHandle::from_raw_handle(stop) });
        let worker_stop = Arc::clone(&stop);
        std::thread::Builder::new()
            .name("oanda-otp".to_owned())
            .spawn(move || {
                if let Err(reason) = watch(&target, &worker_stop) {
                    if unsafe { WaitForMultipleObjects(1, &worker_stop.as_raw_handle(), 0, 0) }
                        == WAIT_TIMEOUT
                    {
                        *target.status.lock() = Status::Message(reason);
                    }
                }
            })
            .ok()?;
        Some(Self { stop })
    }
}

impl Drop for Watcher {
    fn drop(&mut self) {
        unsafe { SetEvent(self.stop.as_raw_handle()) };
    }
}

fn open_target(pid: u32, expected: &Path) -> Option<OwnedHandle> {
    let process = unsafe {
        OpenProcess(
            PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE,
            0,
            pid,
        )
    };
    if process.is_null() {
        return None;
    }
    let process = unsafe { OwnedHandle::from_raw_handle(process) };
    let mut image = [0u16; 1024];
    let mut len = image.len() as u32;
    if unsafe {
        QueryFullProcessImageNameW(process.as_raw_handle(), 0, image.as_mut_ptr(), &mut len)
    } == 0
    {
        return None;
    }
    let image = PathBuf::from(std::ffi::OsString::from_wide(&image[..len as usize]));
    (image.canonicalize().ok()? == expected.canonicalize().ok()?).then_some(process)
}

thread_local! {
    static DIALOG_CHANGED: Cell<bool> = const { Cell::new(false) };
    static HANDLED_DIALOG: Cell<Option<HWND>> = const { Cell::new(None) };
}

unsafe extern "system" fn dialog_event(
    _hook: HWINEVENTHOOK,
    event: u32,
    window: HWND,
    object: i32,
    child: i32,
    _thread: u32,
    _time: u32,
) {
    if object != OBJID_WINDOW || child != CHILDID_SELF as i32 {
        return;
    }
    if event == EVENT_OBJECT_HIDE && HANDLED_DIALOG.get() == Some(window) {
        HANDLED_DIALOG.set(None);
        DIALOG_CHANGED.set(true);
    } else if event == EVENT_OBJECT_SHOW && dialog::is_login_window(window) {
        DIALOG_CHANGED.set(true);
    }
}

struct Hook(HWINEVENTHOOK);

impl Drop for Hook {
    fn drop(&mut self) {
        unsafe { UnhookWinEvent(self.0) };
    }
}

enum Attempt {
    Wait(Duration),
    Sent(Option<MonoNs>),
}

fn attempt(
    target: &Target,
    window: HWND,
    alive: impl Fn() -> bool,
) -> Result<Attempt, &'static str> {
    let credential = Credential::load()?.ok_or("TOTPキーが登録されていません。")?;
    let (account, server) = credential
        .identity()
        .ok_or("登録した口座情報を読み取れません。TOTPキーを登録し直してください。")?;
    let dialog = dialog::LoginDialog::validate(window, target.pid, &account, &server)?;
    let live_tick_before = live_tick(&target.exchange.load_latest(), target.broker_id);
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| "Windowsの時計を確認してください。")?
        .as_secs();
    let remaining = credentials::PERIOD - seconds % credentials::PERIOD;
    if remaining <= 5 {
        return Ok(Attempt::Wait(Duration::from_secs(remaining)));
    }
    let code = credential
        .code(seconds)
        .ok_or("TOTPコードを生成できませんでした。")?;
    drop(credential);
    if !dialog.submit(&code, target.pid, &account, &server, alive) {
        return Err("OTPを送信できませんでした。MT5で手動ログインしてください。");
    }
    Ok(Attempt::Sent(live_tick_before))
}

fn watch(target: &Target, stop: &OwnedHandle) -> Result<(), &'static str> {
    let process = open_target(target.pid, &target.exe)
        .ok_or("OANDA MT5の実行ファイルを確認できませんでした。")?;
    let handles = [stop.as_raw_handle(), process.as_raw_handle()];
    let alive = || unsafe { WaitForMultipleObjects(2, handles.as_ptr(), 0, 0) } == WAIT_TIMEOUT;
    let hook = unsafe {
        SetWinEventHook(
            EVENT_OBJECT_SHOW,
            EVENT_OBJECT_HIDE,
            null_mut(),
            Some(dialog_event),
            target.pid,
            0,
            WINEVENT_OUTOFCONTEXT,
        )
    };
    if hook.is_null() {
        return Err("MT5のOTP画面を監視できませんでした。");
    }
    let _hook = Hook(hook);

    // Cover dialogs already open when the watcher attaches. Subsequent checks
    // are triggered by window events and are bounded while a dialog initializes.
    let mut next_probe = Some(Instant::now());
    let mut probe_until = Instant::now() + Duration::from_secs(2);
    let mut last_attempt = None::<Instant>;
    let mut confirm = None::<(HWND, Instant)>;
    while alive() {
        let now = Instant::now();
        if DIALOG_CHANGED.replace(false) {
            next_probe = Some(now + Duration::from_millis(150));
            probe_until = now + Duration::from_secs(2);
        }
        if next_probe.is_some_and(|next| now >= next) {
            next_probe = None;
            match dialog::login_window(target.pid) {
                Ok(Some(window)) if Some(window) != HANDLED_DIALOG.get() => {
                    if last_attempt.is_some_and(|previous| {
                        now.duration_since(previous) < Duration::from_secs(60)
                    }) {
                        HANDLED_DIALOG.set(Some(window));
                        *target.status.lock() = Status::Message(
                            "MT5が再度認証を求めています。手動でログインしてください。",
                        );
                    } else {
                        match attempt(target, window, alive) {
                            Ok(Attempt::Wait(delay)) => next_probe = Some(now + delay),
                            Ok(Attempt::Sent(live_tick_before)) => {
                                HANDLED_DIALOG.set(Some(window));
                                last_attempt = Some(Instant::now());
                                confirm = Some((window, Instant::now() + Duration::from_secs(15)));
                                *target.status.lock() = Status::Sent { live_tick_before };
                            }
                            Err(reason) if alive() => {
                                HANDLED_DIALOG.set(Some(window));
                                *target.status.lock() = Status::Message(reason);
                            }
                            Err(_) => return Ok(()),
                        }
                    }
                }
                Ok(None) if now < probe_until => {
                    next_probe = Some(now + Duration::from_millis(250));
                }
                Err(reason) => *target.status.lock() = Status::Message(reason),
                _ => {}
            }
        }
        if let Some((window, at)) = confirm {
            if now >= at {
                confirm = None;
                let mut status = target.status.lock();
                if matches!(*status, Status::Sent { .. })
                    && dialog::belongs_to(window, target.pid)
                    && unsafe { IsWindowVisible(window) } != 0
                {
                    *status = Status::Message(
                        "MT5の認証画面が残っています。ログイン結果を確認してください。",
                    );
                }
            }
        }
        let timeout = next_probe
            .into_iter()
            .chain(confirm.map(|(_, at)| at))
            .min()
            .map_or(INFINITE, |at| {
                (at.saturating_duration_since(Instant::now()).as_millis() + 1)
                    .min(u128::from(INFINITE - 1)) as u32
            });
        match unsafe {
            MsgWaitForMultipleObjectsEx(
                2,
                handles.as_ptr(),
                timeout,
                QS_ALLINPUT,
                MWMO_INPUTAVAILABLE,
            )
        } {
            WAIT_OBJECT_0 | 1 => return Ok(()),
            2 => {
                let mut msg = unsafe { std::mem::zeroed() };
                while unsafe { PeekMessageW(&mut msg, null_mut(), 0, 0, PM_REMOVE) } != 0 {
                    unsafe {
                        TranslateMessage(&msg);
                        DispatchMessageW(&msg);
                    }
                }
            }
            WAIT_TIMEOUT => {}
            _ => return Err("MT5のOTP画面の監視を続行できませんでした。"),
        }
    }
    Ok(())
}
