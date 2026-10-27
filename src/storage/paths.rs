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
