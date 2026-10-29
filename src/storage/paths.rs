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
    Logs,
    Core,
    Backups,
}

#[derive(Clone, Debug)]
pub(crate) struct AppPaths {
    config: PathBuf,
    data: PathBuf,
    cache: PathBuf,
}
impl AppPaths {
    pub(crate) fn discover() -> Result<Self, StorageError> {
        Ok(Self {
            config: dirs::config_dir()
                .ok_or(StorageError::MissingDirectory)?
                .join("suki-clash"),
            data: dirs::data_local_dir()
                .ok_or(StorageError::MissingDirectory)?
                .join("suki-clash"),
            cache: dirs::cache_dir()
                .ok_or(StorageError::MissingDirectory)?
                .join("suki-clash"),
        })
    }
    #[cfg(test)]
    pub(crate) fn isolated(root: &Path) -> Self {
        Self {
            config: root.join("config"),
            data: root.join("data"),
            cache: root.join("cache"),
        }
    }
    pub(crate) fn directory(&self, kind: DirectoryKind) -> PathBuf {
        match kind {
            DirectoryKind::Config => self.config.clone(),
            DirectoryKind::Data => self.data.clone(),
            DirectoryKind::Cache => self.cache.clone(),
            DirectoryKind::Logs => self.data.join("logs"),
            DirectoryKind::Core => self.data.join("core"),
            DirectoryKind::Backups => self.data.join("backups"),
        }
    }
    pub(crate) fn settings(&self) -> PathBuf {
        self.config.join("settings")
    }
    pub(crate) fn records(&self) -> PathBuf {
        self.data.join("records")
    }
    pub(crate) fn data(&self) -> &Path {
        &self.data
    }
    /// Called only after both application writers and the owned core have stopped.
    pub(crate) fn reset_configuration(&self) -> Result<(), String> {
        for path in [
            self.settings(),
            self.records(),
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
        let source = self.config.join("runtime-source.yaml");
        match fs::remove_file(&source) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(format!(
                "{}：删除配置失败：{error}；重置未完成",
                source.display()
            )),
        }
    }
    pub(crate) fn create(&self) -> Result<(), StorageError> {
        for kind in [
            DirectoryKind::Config,
            DirectoryKind::Data,
            DirectoryKind::Cache,
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
        fs::create_dir_all(self.records())?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            for directory in [self.settings(), self.records()] {
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
            paths.records(),
            paths.data.join("profiles"),
            paths.data.join("overrides"),
        ];
        for directory in &deleted {
            fs::create_dir_all(directory).unwrap();
            fs::write(directory.join("user-config"), b"configuration").unwrap();
        }
        let source = paths.config.join("runtime-source.yaml");
        fs::write(&source, b"configuration").unwrap();
        let kept = [
            DirectoryKind::Core,
            DirectoryKind::Logs,
            DirectoryKind::Backups,
            DirectoryKind::Cache,
        ]
        .map(|kind| paths.directory(kind).join("keep"));
        for path in &kept {
            fs::write(path, b"keep").unwrap();
        }
        paths.reset_configuration().unwrap();
        assert!(deleted.iter().all(|path| !path.exists()));
        assert!(!source.exists());
        for path in &kept {
            assert_eq!(fs::read(path).unwrap(), b"keep");
        }
        // Reopening uses current defaults, with no subscription or override index.
        let repository = crate::storage::Repository::open(paths.clone()).unwrap();
        crate::preferences::initialize(&repository).unwrap();
        assert!(paths.settings().join("settings.json").is_file());
        assert!(!paths.records().join("profile-index.json").exists());
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
