//! Registration is performed before committing settings; failed commits restore OS intent.
#[cfg(all(windows, not(test)))]
#[path = "autostart_windows.rs"]
mod windows;
#[cfg(all(not(windows), not(test)))]
fn registration() -> Result<auto_launch::AutoLaunch, String> {
    let executable = super::executable()?;
    let path = executable.to_str().ok_or("程序路径不是有效 UTF-8")?;
    let mut builder = auto_launch::AutoLaunchBuilder::new();
    builder
        .set_app_name("Suki Clash")
        .set_args(&["--autostart"]);
    #[cfg(target_os = "linux")]
    builder.set_app_path(&super::desktop_command(path)?);
    #[cfg(not(target_os = "linux"))]
    builder.set_app_path(path);
    #[cfg(target_os = "macos")]
    builder.set_macos_launch_mode(auto_launch::MacOSLaunchMode::LaunchAgent);
    builder.build().map_err(crate::logging::error_message)
}
pub(crate) fn apply(enabled: bool) -> Result<(), String> {
    #[cfg(all(windows, not(test)))]
    {
        windows::apply(enabled)
    }
    #[cfg(all(not(windows), not(test)))]
    {
        let registration = registration()?;
        if enabled {
            registration.enable()
        } else {
            registration.disable()
        }
        .map_err(|error| format!("自启动设置未应用：{error}"))?;
        if registration
            .is_enabled()
            .map_err(crate::logging::error_message)?
            != enabled
        {
            return Err("自启动设置回读不一致".into());
        }
        Ok(())
    }
    #[cfg(test)]
    {
        TEST_ENABLED.set(enabled);
        Ok(())
    }
}
pub(crate) fn enabled() -> Result<bool, String> {
    #[cfg(all(windows, not(test)))]
    {
        windows::enabled()
    }
    #[cfg(all(not(windows), not(test)))]
    {
        registration()?
            .is_enabled()
            .map_err(crate::logging::error_message)
    }
    #[cfg(test)]
    {
        Ok(TEST_ENABLED.get())
    }
}

#[cfg(all(windows, not(test)))]
pub(crate) use windows::Checkpoint;
#[cfg(any(not(windows), test))]
pub(crate) struct Checkpoint(bool);
pub(crate) fn checkpoint() -> Result<Checkpoint, String> {
    #[cfg(all(windows, not(test)))]
    {
        windows::checkpoint()
    }
    #[cfg(any(not(windows), test))]
    {
        Ok(Checkpoint(enabled()?))
    }
}
#[cfg(any(not(windows), test))]
impl Checkpoint {
    pub(crate) fn restore(&self) -> Result<(), String> {
        apply(self.0)
    }
}

/// Reset removes application preferences only after the GUI login entry is disabled.
/// The privileged helper has a separate installation lifecycle and is not touched.
pub(crate) fn reset_configuration(paths: &crate::storage::AppPaths) -> Result<(), String> {
    let previous = checkpoint()?;
    if let Err(cause) = apply(false).and_then(|()| paths.reset_configuration()) {
        return Err(match previous.restore() {
            Ok(()) => cause,
            Err(recovery) => format!("{cause}；自启动恢复失败：{recovery}"),
        });
    }
    Ok(())
}

#[cfg(test)]
thread_local! { static TEST_ENABLED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) }; }
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reset_clears_gui_registration_and_preserves_it_when_reset_fails() {
        let root = tempfile::tempdir().unwrap();
        let paths = crate::storage::AppPaths::isolated(root.path());
        crate::storage::Repository::open(paths.clone()).unwrap();
        apply(true).unwrap();
        reset_configuration(&paths).unwrap();
        assert!(!enabled().unwrap());
        std::fs::create_dir_all(paths.settings().parent().unwrap()).unwrap();
        std::fs::write(paths.settings(), b"not a directory").unwrap();
        apply(true).unwrap();
        assert!(reset_configuration(&paths).is_err());
        assert!(enabled().unwrap());
        apply(false).unwrap();
    }
}
