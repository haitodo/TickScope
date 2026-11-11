use super::credentials::OtpCode;
use windows_sys::Win32::Foundation::{BOOL, HWND, LPARAM};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::IsWindowEnabled;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetAncestor, GetClassNameW, GetDlgItem, GetWindowTextW, GetWindowThreadProcessId,
    IsWindowVisible, PostMessageW, SendMessageTimeoutW, GA_ROOT, IDOK, SMTO_ABORTIFHUNG,
    SMTO_BLOCK, SMTO_ERRORONEXIT, WM_COMMAND, WM_GETTEXT, WM_GETTEXTLENGTH, WM_SETTEXT,
};

// IDs of the native controls exposed by OANDA's MT5 login dialog.
const ACCOUNT: i32 = 10_137;
const PASSWORD: i32 = 10_138;
const SERVER: i32 = 10_139;
const ONE_TIME_PASSWORD: i32 = 10_631;

pub(super) fn belongs_to(hwnd: HWND, pid: u32) -> bool {
    let mut owner = 0;
    !hwnd.is_null() && unsafe { GetWindowThreadProcessId(hwnd, &mut owner) } != 0 && owner == pid
}

fn class_is(hwnd: HWND, class: &str) -> bool {
    let mut buffer = [0u16; 64];
    let len = unsafe { GetClassNameW(hwnd, buffer.as_mut_ptr(), buffer.len() as i32) };
    len > 0
        && buffer[..len as usize]
            .iter()
            .copied()
            .eq(class.encode_utf16())
}

fn is_dialog_window(hwnd: HWND) -> bool {
    !hwnd.is_null() && unsafe { GetAncestor(hwnd, GA_ROOT) } == hwnd && class_is(hwnd, "#32770")
}

pub(super) fn is_login_window(hwnd: HWND) -> bool {
    if !is_dialog_window(hwnd) || unsafe { IsWindowVisible(hwnd) } == 0 {
        return false;
    }
    let mut title = [0u16; 32];
    let len = unsafe { GetWindowTextW(hwnd, title.as_mut_ptr(), title.len() as i32) };
    len > 0
        && ["ログイン", "Login"].iter().any(|name| {
            title[..len as usize]
                .iter()
                .copied()
                .eq(name.encode_utf16())
        })
}

pub(super) fn login_window(pid: u32) -> Result<Option<HWND>, &'static str> {
    struct Windows {
        pid: u32,
        login: Option<HWND>,
        multiple: bool,
    }
    unsafe extern "system" fn collect(hwnd: HWND, context: LPARAM) -> BOOL {
        let windows = unsafe { &mut *(context as *mut Windows) };
        if belongs_to(hwnd, windows.pid)
            && is_login_window(hwnd)
            && class_is(unsafe { GetDlgItem(hwnd, ONE_TIME_PASSWORD) }, "Edit")
            && windows.login.replace(hwnd).is_some()
        {
            windows.multiple = true;
            return 0;
        }
        1
    }
    let mut windows = Windows {
        pid,
        login: None,
        multiple: false,
    };
    unsafe { EnumWindows(Some(collect), std::ptr::from_mut(&mut windows) as LPARAM) };
    if windows.multiple {
        Err("MT5のログイン画面が複数あります。手動でログインしてください。")
    } else {
        Ok(windows.login)
    }
}

fn message(hwnd: HWND, msg: u32, wparam: usize, lparam: LPARAM) -> Option<usize> {
    let mut result = 0;
    let sent = unsafe {
        SendMessageTimeoutW(
            hwnd,
            msg,
            wparam,
            lparam,
            SMTO_ABORTIFHUNG | SMTO_BLOCK | SMTO_ERRORONEXIT,
            250,
            &mut result,
        )
    };
    (sent != 0).then_some(result)
}

fn text_is(hwnd: HWND, expected: &str, ignore_case: bool) -> bool {
    let mut buffer = [0u16; 514];
    message(
        hwnd,
        WM_GETTEXT,
        buffer.len(),
        buffer.as_mut_ptr() as LPARAM,
    )
    .and_then(|len| buffer.get(..len))
    .and_then(|text| String::from_utf16(text).ok())
    .is_some_and(|text| {
        if ignore_case {
            text.trim().eq_ignore_ascii_case(expected)
        } else {
            text.trim() == expected
        }
    })
}

pub(super) struct LoginDialog {
    window: HWND,
    login: HWND,
    server: HWND,
    otp: HWND,
    button: HWND,
}

impl LoginDialog {
    pub(super) fn validate(
        window: HWND,
        pid: u32,
        account: &str,
        server: &str,
    ) -> Result<Self, &'static str> {
        const UNSUPPORTED: &str = "MT5のログイン画面を確認できませんでした。";
        let control = |id, class| {
            let hwnd = unsafe { GetDlgItem(window, id) };
            (belongs_to(hwnd, pid) && class_is(hwnd, class)).then_some(hwnd)
        };
        if !belongs_to(window, pid) || !is_login_window(window) {
            return Err(UNSUPPORTED);
        }
        let login = control(ACCOUNT, "ComboBox").ok_or(UNSUPPORTED)?;
        let password = control(PASSWORD, "Edit").ok_or(UNSUPPORTED)?;
        let server_field = control(SERVER, "ComboBox").ok_or(UNSUPPORTED)?;
        let otp = control(ONE_TIME_PASSWORD, "Edit").ok_or(UNSUPPORTED)?;
        let button = control(IDOK, "Button").ok_or(UNSUPPORTED)?;
        if !text_is(login, account, false) || !text_is(server_field, server, true) {
            return Err("MT5の口座またはサーバーが、登録した情報と一致しません。");
        }
        // MT5 may prefill its password even when the save checkbox is clear.
        // Check whether the field is empty without requesting its value.
        if !message(password, WM_GETTEXTLENGTH, 0, 0).is_some_and(|len| len > 0) {
            return Err("MT5で「パスワードを保存」を有効にしてログインしてください。");
        }
        if [window, otp, button]
            .into_iter()
            .any(|hwnd| unsafe { IsWindowVisible(hwnd) == 0 || IsWindowEnabled(hwnd) == 0 })
            || message(otp, WM_GETTEXTLENGTH, 0, 0) != Some(0)
        {
            return Err("OTP欄またはログインボタンを操作できません。MT5を確認してください。");
        }
        Ok(Self {
            window,
            login,
            server: server_field,
            otp,
            button,
        })
    }

    pub(super) fn submit(
        &self,
        code: &OtpCode,
        pid: u32,
        account: &str,
        server: &str,
        can_submit: impl Fn() -> bool,
    ) -> bool {
        // WM_COMMAND is the notification produced by the dialog's OK button.
        // The MT5 dialog continues to perform the normal broker authentication.
        let dialog_is_current = || {
            can_submit()
                && belongs_to(self.window, pid)
                && is_login_window(self.window)
                && unsafe {
                    IsWindowEnabled(self.window) != 0
                        && GetDlgItem(self.window, ONE_TIME_PASSWORD) == self.otp
                        && GetDlgItem(self.window, IDOK) == self.button
                }
        };
        dialog_is_current()
            && message(self.otp, WM_SETTEXT, 0, code.as_ptr() as LPARAM).is_some_and(|r| r != 0)
            && text_is(self.login, account, false)
            && text_is(self.server, server, true)
            && dialog_is_current()
            && unsafe {
                PostMessageW(
                    self.window,
                    WM_COMMAND,
                    IDOK as usize,
                    self.button as LPARAM,
                )
            } != 0
    }
}
