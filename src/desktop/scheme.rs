//! Platform registration outside GPUI on backends whose official method is unavailable.
#[cfg(not(target_os = "macos"))]
pub(crate) fn register() -> Result<(), String> {
    let executable = super::executable()?;
    let executable = executable.to_str().ok_or("程序路径不是有效 UTF-8")?;
    #[cfg(windows)]
    {
        use windows_sys::Win32::System::Registry::{
            HKEY_CURRENT_USER, KEY_WRITE, REG_SZ, RegCloseKey, RegCreateKeyExW, RegSetValueExW,
        };
        fn write(path: &str, name: &str, value: &str) -> Result<(), String> {
            let path = path.encode_utf16().chain(Some(0)).collect::<Vec<_>>();
            let name = name.encode_utf16().chain(Some(0)).collect::<Vec<_>>();
            let value = value.encode_utf16().chain(Some(0)).collect::<Vec<_>>();
            let mut key = std::ptr::null_mut();
            let status = unsafe {
                RegCreateKeyExW(
                    HKEY_CURRENT_USER,
                    path.as_ptr(),
                    0,
                    std::ptr::null(),
                    0,
                    KEY_WRITE,
                    std::ptr::null(),
                    &mut key,
                    std::ptr::null_mut(),
                )
            };
            if status != 0 {
                return Err(format!("无法注册订阅协议（{status}）"));
            }
            let result = unsafe {
                RegSetValueExW(
                    key,
                    name.as_ptr(),
                    0,
                    REG_SZ,
                    value.as_ptr().cast(),
                    (value.len() * 2) as u32,
                )
            };
            unsafe {
                RegCloseKey(key);
            }
            if result == 0 {
                Ok(())
            } else {
                Err(format!("无法保存订阅协议（{result}）"))
            }
        }
        for scheme in ["clash", "suki-clash"] {
            let path = format!(r"Software\Classes\{scheme}");
            write(&path, "", "URL:Suki Clash subscription")?;
            write(&path, "URL Protocol", "")?;
            write(
                &format!(r"{path}\DefaultIcon"),
                "",
                &format!("\"{executable}\",0"),
            )?;
            write(
                &format!(r"{path}\shell\open\command"),
                "",
                &format!("\"{executable}\" \"%1\""),
            )?;
        }
        Ok(())
    }
    #[cfg(target_os = "linux")]
    {
        let directory = dirs::data_local_dir()
            .ok_or("无法确定桌面应用目录")?
            .join("applications");
        std::fs::create_dir_all(&directory).map_err(crate::logging::error_message)?;
        let icon_directory = directory
            .parent()
            .ok_or("桌面应用目录无效")?
            .join("icons/hicolor/256x256/apps");
        std::fs::create_dir_all(&icon_directory).map_err(crate::logging::error_message)?;
        crate::storage::atomic_write(
            &icon_directory.join("suki-clash.png"),
            include_bytes!("../../assets/logo.png"),
        )
        .map_err(crate::logging::error_message)?;
        let command = super::desktop_command(executable)?;
        let entry = format!(
            "[Desktop Entry]\nType=Application\nName=Suki Clash\nExec={command} %u\nTerminal=false\nIcon=suki-clash\nStartupWMClass=suki-clash\nNoDisplay=true\nMimeType=x-scheme-handler/clash;x-scheme-handler/suki-clash;\n"
        );
        crate::storage::atomic_write(
            &directory.join("dev.suki.clash-uri.desktop"),
            entry.as_bytes(),
        )
        .map_err(crate::logging::error_message)?;
        for scheme in ["clash", "suki-clash"] {
            let status = std::process::Command::new("xdg-mime")
                .args([
                    "default",
                    "dev.suki.clash-uri.desktop",
                    &format!("x-scheme-handler/{scheme}"),
                ])
                .status()
                .map_err(|error| format!("无法注册订阅协议：{error}"))?;
            if !status.success() {
                return Err(format!("订阅协议注册未完成（{status}）"));
            }
        }
        Ok(())
    }
}
