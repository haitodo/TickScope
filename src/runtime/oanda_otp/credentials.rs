use std::ffi::c_void;
use std::ptr::{null, null_mut, NonNull};
use std::sync::atomic::{compiler_fence, Ordering};
use windows_sys::core::w;
use windows_sys::Win32::Foundation::{GetLastError, ERROR_CANCELLED, ERROR_NOT_FOUND, HWND};
use windows_sys::Win32::Security::Credentials::{
    CredDeleteW, CredFree, CredReadW, CredUIPromptForCredentialsW, CredWriteW, CREDENTIALW,
    CREDUI_FLAGS_ALWAYS_SHOW_UI, CREDUI_FLAGS_DO_NOT_PERSIST, CREDUI_FLAGS_GENERIC_CREDENTIALS,
    CREDUI_INFOW, CREDUI_MAX_USERNAME_LENGTH, CRED_PERSIST_LOCAL_MACHINE, CRED_TYPE_GENERIC,
};
use windows_sys::Win32::Security::Cryptography::{
    BCryptCloseAlgorithmProvider, BCryptHash, BCryptOpenAlgorithmProvider,
    BCRYPT_ALG_HANDLE_HMAC_FLAG, BCRYPT_SHA1_ALGORITHM,
};

const TARGET: *const u16 = w!("TickScope/OANDA-MT5/TOTP");
const MAX_KEY_INPUT: usize = 256;
pub(super) const PERIOD: u64 = 30;

fn wipe<T: Copy + Default>(buffer: &mut [T]) {
    for value in buffer {
        // Do not let the compiler remove a write just before freeing the buffer.
        unsafe { std::ptr::write_volatile(value, T::default()) };
    }
    compiler_fence(Ordering::SeqCst);
}

struct Secret<T: Copy + Default, const N: usize>([T; N]);

impl<T: Copy + Default, const N: usize> Drop for Secret<T, N> {
    fn drop(&mut self) {
        wipe(&mut self.0);
    }
}

pub(super) struct Credential(NonNull<CREDENTIALW>);

impl Credential {
    pub(super) fn load() -> Result<Option<Self>, &'static str> {
        let mut credential = null_mut();
        if unsafe { CredReadW(TARGET, CRED_TYPE_GENERIC, 0, &mut credential) } == 0 {
            return if unsafe { GetLastError() } == ERROR_NOT_FOUND {
                Ok(None)
            } else {
                Err("Windows資格情報を読み取れませんでした。")
            };
        }
        Ok(NonNull::new(credential).map(Self))
    }

    pub(super) fn identity(&self) -> Option<(String, String)> {
        let username = unsafe { self.0.as_ref().UserName };
        if username.is_null() {
            return None;
        }
        // Credential Manager returns a NUL-terminated string in its allocation.
        let len =
            (0..CREDUI_MAX_USERNAME_LENGTH as usize).find(|&i| unsafe { *username.add(i) == 0 })?;
        let identity =
            String::from_utf16(unsafe { std::slice::from_raw_parts(username, len) }).ok()?;
        let (account, server) = parse_identity(&identity)?;
        Some((account.to_owned(), server.to_owned()))
    }

    pub(super) fn code(&self, seconds: u64) -> Option<OtpCode> {
        let credential = unsafe { self.0.as_ref() };
        let len = credential.CredentialBlobSize as usize;
        if credential.CredentialBlob.is_null() || !(1..=MAX_KEY_INPUT).contains(&len) {
            return None;
        }
        let key = unsafe { std::slice::from_raw_parts(credential.CredentialBlob, len) };
        totp(key, seconds)
    }
}

impl Drop for Credential {
    fn drop(&mut self) {
        unsafe {
            let credential = self.0.as_ref();
            if !credential.CredentialBlob.is_null() {
                wipe(std::slice::from_raw_parts_mut(
                    credential.CredentialBlob,
                    credential.CredentialBlobSize as usize,
                ));
            }
            CredFree(self.0.as_ptr().cast::<c_void>());
        }
    }
}

fn parse_identity(identity: &str) -> Option<(&str, &str)> {
    let (account, server) = identity.split_once('@')?;
    let (account, server) = (account.trim(), server.trim());
    (!account.is_empty()
        && account.len() <= 20
        && account.bytes().all(|b| b.is_ascii_digit())
        && server
            .get(..5)
            .is_some_and(|s| s.eq_ignore_ascii_case("OANDA"))
        && !server.contains('@'))
    .then_some((account, server))
}

pub(super) fn prompt_and_save(owner: HWND) -> Result<bool, &'static str> {
    let info = CREDUI_INFOW {
        cbSize: std::mem::size_of::<CREDUI_INFOW>() as u32,
        hwndParent: owner,
        pszMessageText: w!(
            "ユーザー名: 口座番号@サーバー名\nパスワード: TOTPセットアップキー（Base32）"
        ),
        pszCaptionText: w!("TickScope - OANDA MT5 自動ログイン"),
        hbmBanner: null_mut(),
    };
    let mut username = [0u16; CREDUI_MAX_USERNAME_LENGTH as usize + 1];
    let mut password = Secret([0u16; MAX_KEY_INPUT + 1]);
    let error = unsafe {
        CredUIPromptForCredentialsW(
            &info,
            w!("TickScope - OANDA MT5"),
            null(),
            0,
            username.as_mut_ptr(),
            username.len() as u32,
            password.0.as_mut_ptr(),
            password.0.len() as u32,
            null_mut(),
            CREDUI_FLAGS_GENERIC_CREDENTIALS
                | CREDUI_FLAGS_ALWAYS_SHOW_UI
                | CREDUI_FLAGS_DO_NOT_PERSIST,
        )
    };
    match error {
        0 => {}
        ERROR_CANCELLED => return Ok(false),
        _ => return Err("Windowsのキー登録画面を開けませんでした。"),
    }

    let len = username
        .iter()
        .position(|&c| c == 0)
        .unwrap_or(username.len());
    let identity = String::from_utf16(&username[..len]).unwrap_or_default();
    let (account, server) = parse_identity(&identity)
        .ok_or("ユーザー名は「口座番号@OANDAのサーバー名」の形式で入力してください。")?;
    let mut username: Vec<u16> = format!("{account}@{server}")
        .encode_utf16()
        .chain([0])
        .collect();
    let mut encoded = Secret([0u8; MAX_KEY_INPUT]);
    let mut key = Secret([0u8; MAX_KEY_INPUT]);
    let len = decode_key(&password.0, &mut encoded.0, &mut key.0)
        .ok_or("パスワード欄にはBase32のTOTPセットアップキーを入力してください。")?;
    drop(password);
    drop(encoded);

    let credential = CREDENTIALW {
        Type: CRED_TYPE_GENERIC,
        TargetName: TARGET.cast_mut(),
        UserName: username.as_mut_ptr(),
        CredentialBlob: key.0.as_mut_ptr(),
        CredentialBlobSize: len as u32,
        Persist: CRED_PERSIST_LOCAL_MACHINE,
        ..unsafe { std::mem::zeroed() }
    };
    if unsafe { CredWriteW(&credential, 0) } == 0 {
        return Err("Windows資格情報にTOTPキーを保存できませんでした。");
    }
    Ok(true)
}

pub(super) fn remove() -> Result<(), &'static str> {
    if unsafe { CredDeleteW(TARGET, CRED_TYPE_GENERIC, 0) } != 0
        || unsafe { GetLastError() } == ERROR_NOT_FOUND
    {
        Ok(())
    } else {
        Err("Windows資格情報からTOTPキーを削除できませんでした。")
    }
}

fn decode_key(input: &[u16], encoded: &mut [u8], key: &mut [u8]) -> Option<usize> {
    let mut len = 0;
    for &unit in input.iter().take_while(|&&unit| unit != 0) {
        let byte = u8::try_from(unit).ok()?;
        if !byte.is_ascii_whitespace() {
            *encoded.get_mut(len)? = byte.to_ascii_uppercase();
            len += 1;
        }
    }
    let encoded = &encoded[..len];
    let encoding = if encoded.ends_with(b"=") {
        data_encoding::BASE32
    } else {
        data_encoding::BASE32_NOPAD
    };
    let capacity = encoding.decode_len(len).ok()?;
    let len = encoding
        .decode_mut(encoded, key.get_mut(..capacity)?)
        .ok()?;
    (len > 0).then_some(len)
}

pub(super) struct OtpCode {
    digits: Secret<u16, 7>,
}

impl OtpCode {
    pub(super) fn as_ptr(&self) -> *const u16 {
        self.digits.0.as_ptr()
    }
}

fn totp(key: &[u8], seconds: u64) -> Option<OtpCode> {
    let counter = (seconds / PERIOD).to_be_bytes();
    let mut digest = Secret([0u8; 20]);
    let mut algorithm = null_mut();
    let status = unsafe {
        BCryptOpenAlgorithmProvider(
            &mut algorithm,
            BCRYPT_SHA1_ALGORITHM,
            null(),
            BCRYPT_ALG_HANDLE_HMAC_FLAG,
        )
    };
    if status < 0 {
        return None;
    }
    let status = unsafe {
        let status = BCryptHash(
            algorithm,
            key.as_ptr(),
            key.len() as u32,
            counter.as_ptr(),
            counter.len() as u32,
            digest.0.as_mut_ptr(),
            digest.0.len() as u32,
        );
        BCryptCloseAlgorithmProvider(algorithm, 0);
        status
    };
    if status < 0 {
        return None;
    }
    let start = usize::from(digest.0[19] & 15);
    let mut value = u32::from_be_bytes(digest.0[start..start + 4].try_into().ok()?) & 0x7fff_ffff;
    let mut code = OtpCode {
        digits: Secret([0u16; 7]),
    };
    for digit in code.digits.0[..6].iter_mut().rev() {
        *digit = u16::from(b'0') + (value % 10) as u16;
        value /= 10;
    }
    Some(code)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generates_six_digit_codes_from_rfc6238_sha1_vectors() {
        // The public RFC test key, never a user's credential.
        let input: Vec<u16> = "GEZD GNBV GY3T QOJQ GEZD GNBV GY3T QOJQ"
            .encode_utf16()
            .collect();
        let mut encoded = Secret([0u8; MAX_KEY_INPUT]);
        let mut key = Secret([0u8; MAX_KEY_INPUT]);
        let len = decode_key(&input, &mut encoded.0, &mut key.0).unwrap();
        for (time, expected) in [
            (59, "287082"),
            (1_111_111_109, "081804"),
            (1_111_111_111, "050471"),
            (1_234_567_890, "005924"),
            (2_000_000_000, "279037"),
            (20_000_000_000, "353130"),
        ] {
            let code = totp(&key.0[..len], time).unwrap();
            assert_eq!(String::from_utf16(&code.digits.0[..6]).unwrap(), expected);
        }
    }
}
