//! Current process privilege; shared by diagnostics and core execution policy.
pub(crate) fn elevated() -> Result<bool, String> {
    #[cfg(unix)]
    {
        unsafe extern "C" {
            fn geteuid() -> u32;
        }
        Ok(unsafe { geteuid() } == 0)
    }
    #[cfg(windows)]
    {
        use windows_sys::Win32::{
            Foundation::CloseHandle,
            Security::{GetTokenInformation, TOKEN_ELEVATION, TOKEN_QUERY, TokenElevation},
            System::Threading::{GetCurrentProcess, OpenProcessToken},
        };
        let mut token = std::ptr::null_mut();
        if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0 {
            return Err("无法读取进程权限".into());
        }
        let mut elevation: TOKEN_ELEVATION = unsafe { std::mem::zeroed() };
        let mut length = 0;
        let result = unsafe {
            GetTokenInformation(
                token,
                TokenElevation,
                (&mut elevation as *mut TOKEN_ELEVATION).cast(),
                std::mem::size_of::<TOKEN_ELEVATION>() as u32,
                &mut length,
            )
        };
        unsafe {
            CloseHandle(token);
        }
        if result == 0 {
            Err("无法读取进程权限".into())
        } else {
            Ok(elevation.TokenIsElevated != 0)
        }
    }
}
