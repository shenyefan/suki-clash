use super::{Backend, Target, types::OsSnapshot};
#[cfg(target_os = "linux")]
#[path = "linux.rs"]
mod platform;
#[cfg(target_os = "macos")]
#[path = "macos.rs"]
mod platform;
#[cfg(windows)]
#[path = "windows.rs"]
mod platform;
pub(super) struct Native {
    #[cfg(any(target_os = "macos", windows))]
    adapter: platform::Adapter,
    #[cfg(target_os = "linux")]
    journal: Option<std::path::PathBuf>,
    #[cfg(target_os = "linux")]
    owned: Vec<Target>,
}
impl Native {
    pub(super) fn new(
        paths: Option<&crate::storage::AppPaths>,
        interactive: bool,
    ) -> Result<Self, String> {
        #[cfg(any(target_os = "macos", windows))]
        {
            Ok(Self {
                adapter: platform::Adapter::new(
                    paths.map(|paths| {
                        paths
                            .directory(crate::storage::DirectoryKind::Runtime)
                            .join("system-proxy-owner.json")
                    }),
                    interactive,
                )?,
            })
        }
        #[cfg(target_os = "linux")]
        {
            let _ = interactive;
            let journal = paths.map(|paths| {
                paths
                    .directory(crate::storage::DirectoryKind::Runtime)
                    .join("system-proxy-owner.json")
            });
            let mut native = Self {
                journal,
                owned: Vec::new(),
            };
            if let Some(path) = &native.journal {
                match std::fs::read(path) {
                    Ok(bytes) => {
                        native.owned = serde_json::from_slice(&bytes).map_err(|error| {
                            crate::logging::diagnostic_error(
                                "系统代理恢复记录损坏，未覆盖原文件",
                                error,
                            )
                        })?;
                        for target in native.owned.clone() {
                            native.clear(&target)?;
                        }
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                    Err(error) => return Err(format!("无法读取系统代理恢复记录：{error}")),
                }
            }
            Ok(native)
        }
    }
}
impl Backend for Native {
    type Checkpoint = platform::Checkpoint;
    fn available(&self) -> Result<bool, String> {
        #[cfg(target_os = "macos")]
        {
            self.adapter.available()
        }
        #[cfg(not(target_os = "macos"))]
        {
            Ok(true)
        }
    }
    fn matches(&self, target: &Target, state: &OsSnapshot) -> Result<bool, String> {
        #[cfg(windows)]
        {
            self.adapter.matches(target, state)
        }
        #[cfg(not(windows))]
        {
            Ok(target.matches(state))
        }
    }
    fn checkpoint_matches(
        &self,
        checkpoint: &Self::Checkpoint,
        before: &OsSnapshot,
    ) -> Result<bool, String> {
        #[cfg(windows)]
        {
            self.adapter.checkpoint_matches(checkpoint, before)
        }
        #[cfg(target_os = "linux")]
        {
            let _ = before;
            platform::checkpoint_matches(checkpoint)
        }
        #[cfg(target_os = "macos")]
        {
            self.adapter.checkpoint_matches(checkpoint, before)
        }
    }
    fn capture(&mut self) -> Result<(OsSnapshot, Self::Checkpoint), String> {
        #[cfg(any(target_os = "macos", windows))]
        {
            self.adapter.capture()
        }
        #[cfg(target_os = "linux")]
        {
            platform::capture()
        }
    }
    fn read(&self) -> Result<OsSnapshot, String> {
        platform::read()
    }
    fn write(&mut self, target: &Target) -> Result<(), String> {
        #[cfg(any(target_os = "macos", windows))]
        {
            self.adapter.write(target)
        }
        #[cfg(target_os = "linux")]
        {
            if target.system.enable || target.auto.enable {
                if !self.owned.contains(target) {
                    self.owned.push(target.clone());
                }
                if let Some(path) = &self.journal {
                    if let Some(parent) = path.parent() {
                        std::fs::create_dir_all(parent).map_err(crate::logging::error_message)?;
                    }
                    crate::storage::atomic_write(
                        path,
                        &serde_json::to_vec(&self.owned).map_err(|error| {
                            crate::logging::diagnostic_error("无法编码代理恢复记录", error)
                        })?,
                    )
                    .map_err(crate::logging::error_message)?;
                }
            }
            platform::write(target)
        }
    }
    fn allow_interaction(&mut self, allowed: bool) {
        #[cfg(target_os = "macos")]
        {
            self.adapter.allow_interaction(allowed);
        }
        #[cfg(not(target_os = "macos"))]
        let _ = allowed;
    }
    fn restore(&mut self, checkpoint: &Self::Checkpoint) -> Result<(), String> {
        #[cfg(any(target_os = "macos", windows))]
        {
            self.adapter.restore(checkpoint)
        }
        #[cfg(target_os = "linux")]
        {
            platform::restore(checkpoint)
        }
    }
    fn clear(&mut self, target: &Target) -> Result<OsSnapshot, String> {
        #[cfg(any(target_os = "macos", windows))]
        {
            Backend::clear(&mut self.adapter, target)
        }
        #[cfg(target_os = "linux")]
        {
            let observed = super::clear_current(self, target)?;
            self.owned.retain(|target| target.owns(&observed));
            if let Some(path) = &self.journal {
                crate::storage::atomic_write(
                    path,
                    &serde_json::to_vec(&self.owned).map_err(|error| {
                        crate::logging::diagnostic_error("无法编码代理恢复记录", error)
                    })?,
                )
                .map_err(|error| format!("系统代理已清理；无法更新恢复记录：{error}"))?;
            }
            Ok(observed)
        }
    }
}
