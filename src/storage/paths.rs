use super::StorageError;
use std::{
    fs,
    path::{Path, PathBuf},
};

#[derive(Clone, Copy, Debug)]
pub(crate) enum DirectoryKind {
    Config,
    Data,
    Cache,
    Runtime,
    Logs,
    Core,
    Backups,
}

#[derive(Clone, Debug)]
pub(crate) struct AppPaths {
    config: PathBuf,
    data: PathBuf,
    cache: PathBuf,
    runtime: PathBuf,
    logs: PathBuf,
}
impl AppPaths {
    /// Each kind uses its platform location: local (non-roaming) settings and data,
    /// system cache, runtime and log directories.
    pub(crate) fn discover() -> Result<Self, StorageError> {
        const NAME: &str = "suki-clash";
        let missing = || StorageError::MissingDirectory;
        let config = dirs::config_local_dir().ok_or_else(missing)?.join(NAME);
        let data = dirs::data_local_dir().ok_or_else(missing)?.join(NAME);
        let cache = if cfg!(windows) {
            data.join("cache")
        } else {
            dirs::cache_dir().ok_or_else(missing)?.join(NAME)
        };
        let runtime = if cfg!(target_os = "linux") {
            dirs::runtime_dir().map_or_else(|| cache.clone(), |directory| directory.join(NAME))
        } else {
            cache.clone()
        };
        let logs = if cfg!(target_os = "macos") {
            dirs::home_dir()
                .ok_or_else(missing)?
                .join("Library/Logs")
                .join(NAME)
        } else if cfg!(target_os = "linux") {
            dirs::state_dir()
                .ok_or_else(missing)?
                .join(NAME)
                .join("logs")
        } else {
            data.join("logs")
        };
        Ok(Self {
            config,
            data,
            cache,
            runtime,
            logs,
        })
    }
    pub(crate) fn staged(root: &Path) -> Self {
        Self {
            config: root.join("config"),
            data: root.join("data"),
            cache: root.join("cache"),
            runtime: root.join("runtime"),
            logs: root.join("logs"),
        }
    }
    #[cfg(test)]
    pub(crate) fn isolated(root: &Path) -> Self {
        Self {
            config: root.join("config"),
            data: root.join("data"),
            cache: root.join("cache"),
            runtime: root.join("runtime"),
            logs: root.join("logs"),
        }
    }
    pub(crate) fn directory(&self, kind: DirectoryKind) -> PathBuf {
        match kind {
            DirectoryKind::Config => self.config.clone(),
            DirectoryKind::Data => self.data.clone(),
            DirectoryKind::Cache => self.cache.clone(),
            DirectoryKind::Runtime => self.runtime.clone(),
            DirectoryKind::Logs => self.logs.clone(),
            DirectoryKind::Core => self.data.join("core"),
            DirectoryKind::Backups => self.data.join("backups"),
        }
    }
    pub(crate) fn settings(&self) -> PathBuf {
        self.config.join("settings")
    }
    pub(crate) fn traffic_record(&self) -> PathBuf {
        self.cache.join("traffic.json")
    }
    pub(crate) fn indexes(&self) -> PathBuf {
        self.data.join("indexes")
    }
    pub(crate) fn data(&self) -> &Path {
        &self.data
    }
    /// Called only after both application writers and the owned core have stopped.
    pub(crate) fn reset_configuration(&self) -> Result<(), String> {
        for path in [
            self.settings(),
            self.indexes(),
            self.data.join("profiles"),
            self.data.join("overrides"),
        ] {
            match fs::remove_dir_all(&path) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => {
                    return Err(format!(
                        "{}：删除配置失败：{error}；重置未完成",
                        path.display()
                    ));
                }
            }
        }
        Ok(())
    }
    pub(crate) fn create(&self) -> Result<(), StorageError> {
        for kind in [
            DirectoryKind::Config,
            DirectoryKind::Data,
            DirectoryKind::Cache,
            DirectoryKind::Runtime,
            DirectoryKind::Logs,
            DirectoryKind::Core,
            DirectoryKind::Backups,
        ] {
            let directory = self.directory(kind);
            fs::create_dir_all(&directory)?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(&directory, fs::Permissions::from_mode(0o700))?;
            }
        }
        fs::create_dir_all(self.settings())?;
        fs::create_dir_all(self.indexes())?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            for directory in [self.settings(), self.indexes()] {
                fs::set_permissions(directory, fs::Permissions::from_mode(0o700))?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod reset_tests {
    use super::*;
    #[test]
    fn reset_deletes_configuration_and_keeps_installed_assets_logs_and_backups() {
        let root = tempfile::tempdir().unwrap();
        let paths = AppPaths::isolated(root.path());
        paths.create().unwrap();
        let deleted = [
            paths.settings(),
            paths.indexes(),
            paths.data.join("profiles"),
            paths.data.join("overrides"),
        ];
        for directory in &deleted {
            fs::create_dir_all(directory).unwrap();
            fs::write(directory.join("user-config"), b"configuration").unwrap();
        }
        let kept = [
            DirectoryKind::Core,
            DirectoryKind::Logs,
            DirectoryKind::Backups,
            DirectoryKind::Cache,
            DirectoryKind::Runtime,
        ]
        .map(|kind| paths.directory(kind).join("keep"));
        for path in &kept {
            fs::write(path, b"keep").unwrap();
        }
        paths.reset_configuration().unwrap();
        assert!(deleted.iter().all(|path| !path.exists()));
        for path in &kept {
            assert_eq!(fs::read(path).unwrap(), b"keep");
        }
        // Reopening uses current defaults, with no subscription or override index.
        let repository = crate::storage::Repository::open(paths.clone()).unwrap();
        crate::preferences::initialize(&repository).unwrap();
        assert!(paths.settings().join("settings.json").is_file());
        assert!(!paths.indexes().join("profile-index.json").exists());
    }
    #[test]
    fn deletion_error_reports_incomplete_reset() {
        let root = tempfile::tempdir().unwrap();
        let paths = AppPaths::isolated(root.path());
        fs::create_dir_all(&paths.config).unwrap();
        fs::write(paths.settings(), b"not a directory").unwrap();
        let error = paths.reset_configuration().unwrap_err();
        assert!(error.contains("重置未完成"));
        assert!(paths.settings().exists());
    }
}
