//! D49 layer 3: confirms that the person at the main PC is the Windows user running TallyCheck,
//! for "Forgot password? → Reset using Windows sign-in". Windows Hello (PIN, fingerprint, face)
//! when it is set up; otherwise Windows' own password window, checked with `LogonUserW` and
//! accepted only for the same account (SID) that runs TallyCheck. The password typed there is
//! never kept or logged: it is wiped from memory right after the check.

use api_server::local_verify::{Verification, VerifyFuture, WindowsVerifier};
use tauri::Manager;

pub struct WindowsSignIn {
    pub app: tauri::AppHandle,
}

impl WindowsVerifier for WindowsSignIn {
    fn verify(&self, message: String) -> VerifyFuture {
        // The window handle as a number, so it can cross threads; Windows shows its window over it.
        let hwnd =
            self.app.get_webview_window("main").and_then(|w| w.hwnd().ok()).map(|h| h.0 as isize).unwrap_or_default();
        Box::pin(async move {
            tauri::async_runtime::spawn_blocking(move || imp::verify(hwnd, &message)).await.unwrap_or_else(|e| {
                tracing::error!("Windows verification task failed: {e}");
                Verification::Unavailable("Windows could not check who you are. Try again.".into())
            })
        })
    }
}

#[cfg(windows)]
#[allow(unsafe_code)]
mod imp {
    use api_server::local_verify::Verification;
    use std::ffi::c_void;
    use windows::core::{factory, HSTRING, PCWSTR, PWSTR};
    use windows::Security::Credentials::UI::{
        UserConsentVerificationResult, UserConsentVerifier, UserConsentVerifierAvailability,
    };
    use windows::Win32::Foundation::{CloseHandle, ERROR_CANCELLED, HANDLE, HWND};
    use windows::Win32::Security::Credentials::{
        CredUIPromptForWindowsCredentialsW, CredUnPackAuthenticationBufferW, CREDUIWIN_ENUMERATE_CURRENT_USER,
        CREDUI_INFOW, CRED_PACK_PROTECTED_CREDENTIALS,
    };
    use windows::Win32::Security::{
        GetLengthSid, GetTokenInformation, LogonUserW, TokenUser, LOGON32_LOGON_INTERACTIVE, LOGON32_PROVIDER_DEFAULT,
        TOKEN_QUERY, TOKEN_USER,
    };
    use windows::Win32::System::Com::CoTaskMemFree;
    use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
    use windows::Win32::System::WinRT::{IUserConsentVerifierInterop, RoInitialize, RO_INIT_MULTITHREADED};
    use windows_future::IAsyncOperation;

    pub fn verify(hwnd: isize, message: &str) -> Verification {
        // SAFETY: initialises WinRT for this worker thread; "already initialised" is fine.
        let _ = unsafe { RoInitialize(RO_INIT_MULTITHREADED) };
        match hello(hwnd, message) {
            Ok(Some(answer)) => answer,
            Ok(None) => password(hwnd, message),
            Err(e) => {
                tracing::warn!("Windows Hello unavailable, asking for the Windows password: {e}");
                password(hwnd, message)
            }
        }
    }

    /// Windows Hello. `None` when Hello isn't set up on this PC (then the password is asked).
    fn hello(hwnd: isize, message: &str) -> windows::core::Result<Option<Verification>> {
        if UserConsentVerifier::CheckAvailabilityAsync()?.join()? != UserConsentVerifierAvailability::Available {
            return Ok(None);
        }
        let interop = factory::<UserConsentVerifier, IUserConsentVerifierInterop>()?;
        // SAFETY: `hwnd` is TallyCheck's own main window (or null); the message outlives the call.
        let op: IAsyncOperation<UserConsentVerificationResult> =
            unsafe { interop.RequestVerificationForWindowAsync(HWND(hwnd as *mut c_void), &HSTRING::from(message))? };
        Ok(match op.join()? {
            UserConsentVerificationResult::Verified => Some(Verification::Verified),
            UserConsentVerificationResult::DeviceNotPresent
            | UserConsentVerificationResult::NotConfiguredForUser
            | UserConsentVerificationResult::DisabledByPolicy => None,
            _ => Some(Verification::NotVerified),
        })
    }

    fn wipe(buf: &mut [u16]) {
        for c in buf.iter_mut() {
            // SAFETY: a valid, aligned element of the slice; volatile so the wipe isn't optimised away.
            unsafe { std::ptr::write_volatile(c, 0) };
        }
    }

    /// Windows' own sign-in window for the current user, then `LogonUserW` with what was typed.
    fn password(hwnd: isize, message: &str) -> Verification {
        let caption = HSTRING::from("TallyCheck");
        let text = HSTRING::from(format!("{message}\n\nEnter your Windows password."));
        let info = CREDUI_INFOW {
            cbSize: std::mem::size_of::<CREDUI_INFOW>() as u32,
            hwndParent: HWND(hwnd as *mut c_void),
            pszMessageText: PCWSTR(text.as_ptr()),
            pszCaptionText: PCWSTR(caption.as_ptr()),
            hbmBanner: Default::default(),
        };
        let mut package = 0u32;
        let mut out: *mut c_void = std::ptr::null_mut();
        let mut out_size = 0u32;
        // SAFETY: every pointer refers to a live local; Windows allocates `out` (freed below).
        let rc = unsafe {
            CredUIPromptForWindowsCredentialsW(
                Some(&info),
                0,
                &mut package,
                None,
                0,
                &mut out,
                &mut out_size,
                None,
                CREDUIWIN_ENUMERATE_CURRENT_USER,
            )
        };
        if rc == ERROR_CANCELLED.0 {
            return Verification::NotVerified;
        }
        if rc != 0 || out.is_null() {
            return Verification::Unavailable(format!("Windows could not show its sign-in window (error {rc})."));
        }

        let mut user = [0u16; 514];
        let mut domain = [0u16; 338];
        let mut pass = [0u16; 514];
        let (mut user_len, mut domain_len, mut pass_len) = (user.len() as u32, domain.len() as u32, pass.len() as u32);
        // SAFETY: `out`/`out_size` come from the call above; the buffers and their lengths match.
        let unpacked = unsafe {
            CredUnPackAuthenticationBufferW(
                CRED_PACK_PROTECTED_CREDENTIALS,
                out,
                out_size,
                Some(PWSTR(user.as_mut_ptr())),
                &mut user_len,
                Some(PWSTR(domain.as_mut_ptr())),
                Some(&mut domain_len),
                Some(PWSTR(pass.as_mut_ptr())),
                &mut pass_len,
            )
        };
        // SAFETY: wipes and frees the buffer Windows allocated for us, exactly once.
        unsafe {
            std::ptr::write_bytes(out.cast::<u8>(), 0, out_size as usize);
            CoTaskMemFree(Some(out));
        }
        if unpacked.is_err() {
            wipe(&mut pass);
            return Verification::Unavailable(
                "Please type your Windows password (a PIN can't be checked this way).".into(),
            );
        }

        // "DOMAIN\name" typed as one name: LogonUserW wants the two apart.
        let end = user.iter().position(|&c| c == 0).unwrap_or(user.len());
        let full = String::from_utf16_lossy(&user[..end]);
        let has_domain = domain.first().is_some_and(|&c| c != 0);
        let (name, dom) = match full.split_once('\\') {
            Some((d, n)) if !has_domain => (HSTRING::from(n), Some(HSTRING::from(d))),
            _ => (
                HSTRING::from(full.as_str()),
                has_domain.then(|| HSTRING::from_wide(&domain[..domain.iter().position(|&c| c == 0).unwrap_or(0)])),
            ),
        };
        let mut token = HANDLE::default();
        // SAFETY: NUL-terminated strings that outlive the call; `token` is closed below.
        let logged_on = unsafe {
            LogonUserW(
                &name,
                dom.as_ref().map_or(PCWSTR::null(), |d| PCWSTR(d.as_ptr())),
                PCWSTR(pass.as_ptr()),
                LOGON32_LOGON_INTERACTIVE,
                LOGON32_PROVIDER_DEFAULT,
                &mut token,
            )
        };
        wipe(&mut pass);
        if logged_on.is_err() {
            return Verification::NotVerified;
        }
        let same = same_account(token);
        // SAFETY: `token` was opened by LogonUserW above.
        unsafe {
            let _ = CloseHandle(token);
        }
        if same {
            Verification::Verified
        } else {
            Verification::NotVerified
        }
    }

    /// The SID of a token's user, as bytes.
    fn sid_of(token: HANDLE) -> Option<Vec<u8>> {
        let mut size = 0u32;
        // SAFETY: a size query (no buffer), then a buffer of at least that size. The buffer is
        // made of u64s so it is aligned for TOKEN_USER (pointer-aligned) before it is read as one.
        unsafe {
            let _ = GetTokenInformation(token, TokenUser, None, 0, &mut size);
            if size == 0 {
                return None;
            }
            let mut buf = vec![0u64; (size as usize).div_ceil(std::mem::size_of::<u64>())];
            GetTokenInformation(token, TokenUser, Some(buf.as_mut_ptr().cast()), size, &mut size).ok()?;
            let user = &*(buf.as_ptr().cast::<TOKEN_USER>());
            let len = GetLengthSid(user.User.Sid) as usize;
            Some(std::slice::from_raw_parts(user.User.Sid.0.cast::<u8>(), len).to_vec())
        }
    }

    /// Whether `token` belongs to the Windows account that runs TallyCheck.
    fn same_account(token: HANDLE) -> bool {
        let mut own = HANDLE::default();
        // SAFETY: opens this process's own token for reading and closes it again.
        let mine = unsafe {
            if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut own).is_err() {
                return false;
            }
            let sid = sid_of(own);
            let _ = CloseHandle(own);
            sid
        };
        matches!((mine, sid_of(token)), (Some(a), Some(b)) if a == b)
    }
}

#[cfg(not(windows))]
mod imp {
    use api_server::local_verify::Verification;

    pub fn verify(_hwnd: isize, _message: &str) -> Verification {
        Verification::Unavailable("Windows sign-in is only available on Windows".into())
    }
}
