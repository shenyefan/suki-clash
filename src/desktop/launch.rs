//! Persistent launch targets and freedesktop Exec encoding.
use std::path::PathBuf;

pub(crate) fn executable() -> Result<PathBuf, String> {
    let current = std::env::current_exe().map_err(|error| format!("无法定位程序：{error}"))?;
    #[cfg(target_os = "linux")]
    return resolve_appimage(current, std::env::var_os("APPIMAGE"));
    #[cfg(not(target_os = "linux"))]
    Ok(current)
}

#[cfg(any(target_os = "linux", test))]
fn resolve_appimage(
    current: PathBuf,
    appimage: Option<std::ffi::OsString>,
) -> Result<PathBuf, String> {
    match appimage {
        None => Ok(current),
        Some(path) => {
            let path = PathBuf::from(path);
            if !path.is_absolute() || !path.is_file() {
                return Err("APPIMAGE 必须指向有效的绝对文件路径".into());
            }
            Ok(path)
        }
    }
}

/// Desktop values are unescaped before Exec argument quoting is interpreted.
#[cfg(any(target_os = "linux", test))]
pub(super) fn desktop_command(path: &str) -> Result<String, String> {
    if path.contains('=') {
        return Err("桌面启动路径不能包含等号".into());
    }
    let mut quoted = String::from("\"");
    for ch in path.chars() {
        match ch {
            '\\' => quoted.push_str(r"\\\\"),
            '"' => quoted.push_str("\\\\\""),
            '$' => quoted.push_str(r"\\$"),
            '`' => quoted.push_str(r"\\`"),
            '%' => quoted.push_str("%%"),
            '\n' => quoted.push_str(r"\n"),
            '\r' => quoted.push_str(r"\r"),
            '\t' => quoted.push_str(r"\t"),
            _ => quoted.push(ch),
        }
    }
    quoted.push('"');
    Ok(quoted)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn appimage_registration_and_restart_use_the_persistent_file_not_the_mount() {
        let root = tempfile::tempdir().unwrap();
        let image = root.path().join("Suki Clash.AppImage");
        std::fs::write(&image, b"fixture").unwrap();
        let mounted = PathBuf::from("/tmp/.mount_suki/usr/bin/suki-clash");
        assert_eq!(
            resolve_appimage(mounted.clone(), Some(image.clone().into_os_string())).unwrap(),
            image
        );
        assert_eq!(resolve_appimage(mounted.clone(), None).unwrap(), mounted);
        assert!(resolve_appimage(mounted.clone(), Some("relative.AppImage".into())).is_err());
        assert!(
            resolve_appimage(mounted, Some(root.path().join("absent").into_os_string())).is_err()
        );
    }
    #[test]
    fn desktop_exec_quotes_paths_and_preserves_literal_reserved_characters() {
        assert_eq!(
            desktop_command("/opt/Suki Clash.AppImage").unwrap(),
            "\"/opt/Suki Clash.AppImage\""
        );
        assert_eq!(
            desktop_command("/tmp/a\\b\"$`%.AppImage").unwrap(),
            concat!(
                "\"/tmp/a",
                r"\\\\",
                "b",
                "\\\\\"",
                r"\\$",
                r"\\`",
                "%%.AppImage\""
            )
        );
        assert!(desktop_command("/tmp/a=b").is_err());
    }
}
