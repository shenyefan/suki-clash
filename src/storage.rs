//! Durable application storage. This module has no GPUI dependency.
mod paths;
#[cfg(test)]
mod tests;

pub(crate) use paths::{AppPaths, DirectoryKind};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
};

pub(crate) type Values = BTreeMap<String, String>;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct Namespace(String);
impl Namespace {
    pub(crate) fn parse(name: &str) -> Result<Self, StorageError> {
        if matches!(
            name,
            "settings"
                | "core"
                | "dns"
                | "sniffer"
                | "resources"
                | "sysproxy"
                | "tun"
                | "logs"
                | "proxy-chain"
                | "profile-index"
                | "override-index"
        ) || name
            .strip_prefix("profile-enhance-")
            .is_some_and(|id| id.parse::<u64>().is_ok())
        {
            Ok(Self(name.to_owned()))
        } else {
            Err(StorageError::InvalidNamespace)
        }
    }
    pub(crate) fn name(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum StorageError {
    #[error("无法确定当前用户的应用目录")]
    MissingDirectory,
    #[error("无效的配置存储名称")]
    InvalidNamespace,
    #[error("存储格式不受支持：{0}")]
    UnsupportedVersion(u32),
    #[error("配置文件格式错误：{0}")]
    InvalidDocument(#[from] serde_json::Error),
    #[error("存储操作失败：{0}")]
    Io(#[from] io::Error),
    #[error("不允许迁移符号链接或特殊文件：{0}")]
    UnsafeMigration(PathBuf),
}

#[derive(Serialize, Deserialize)]
struct Envelope {
    version: u32,
    values: Values,
}

pub(crate) struct Repository {
    paths: AppPaths,
}
impl Repository {
    pub(crate) fn open(paths: AppPaths, preview: Option<&Path>) -> Result<Self, StorageError> {
        paths.create()?;
        let store = Self { paths };
        if let Some(preview) = preview {
            store.migrate_preview(preview)?;
        }
        Ok(store)
    }
    pub(crate) fn paths(&self) -> &AppPaths {
        &self.paths
    }
    fn path(&self, namespace: &Namespace) -> PathBuf {
        self.paths
            .settings()
            .join(format!("{}.json", namespace.name()))
    }
    pub(crate) fn read(&self, namespace: &Namespace) -> Result<Values, StorageError> {
        match fs::read(self.path(namespace)) {
            Ok(bytes) => decode(&bytes),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Values::new()),
            Err(error) => Err(error.into()),
        }
    }
    pub(crate) fn snapshot(&self) -> Result<BTreeMap<Namespace, Values>, StorageError> {
        let mut snapshot = BTreeMap::new();
        for entry in fs::read_dir(self.paths.settings())? {
            let path = entry?.path();
            if path.extension().and_then(|value| value.to_str()) != Some("json") {
                continue;
            }
            let Some(name) = path.file_stem().and_then(|value| value.to_str()) else {
                continue;
            };
            let namespace = Namespace::parse(name)?;
            snapshot.insert(namespace.clone(), self.read(&namespace)?);
        }
        Ok(snapshot)
    }
    pub(crate) fn save(&self, namespace: &Namespace, values: &Values) -> Result<(), StorageError> {
        let values = persistent_values(values);
        let envelope = Envelope { version: 1, values };
        atomic_write(
            &self.path(namespace),
            &serde_json::to_vec_pretty(&envelope)?,
        )
    }
    pub(crate) fn backup(&self, namespace: &Namespace) -> Result<PathBuf, StorageError> {
        // Validate before making this the latest recoverable copy.
        let bytes = fs::read(self.path(namespace))?;
        decode(&bytes)?;
        let path = self
            .paths
            .directory(DirectoryKind::Backups)
            .join(format!("{}.json", namespace.name()));
        atomic_write(&path, &bytes)?;
        Ok(path)
    }
    pub(crate) fn restore(&self, namespace: &Namespace) -> Result<Values, StorageError> {
        let bytes = fs::read(
            self.paths
                .directory(DirectoryKind::Backups)
                .join(format!("{}.json", namespace.name())),
        )?;
        decode(&bytes)
    }
    fn migrate_preview(&self, preview: &Path) -> Result<(), StorageError> {
        let marker = self.paths.settings().join(".preview-migrated-v1");
        if marker.exists() || !preview.try_exists()? {
            return Ok(());
        }
        reject_link(preview)?;
        for entry in fs::read_dir(preview)? {
            let path = entry?.path();
            reject_link(&path)?;
            if path.extension().and_then(|value| value.to_str()) == Some("settings") {
                let Some(name) = path.file_stem().and_then(|value| value.to_str()) else {
                    continue;
                };
                let namespace = Namespace::parse(name)?;
                if !self.path(&namespace).try_exists()? {
                    self.save(&namespace, &parse_legacy(&fs::read_to_string(&path)?)?)?;
                }
            } else if path.extension().and_then(|value| value.to_str()) == Some("backup") {
                let Some(name) = path.file_stem().and_then(|value| value.to_str()) else {
                    continue;
                };
                let namespace = Namespace::parse(name)?;
                let target = self
                    .paths
                    .directory(DirectoryKind::Backups)
                    .join(format!("{}.json", namespace.name()));
                if !target.try_exists()? {
                    let values = persistent_values(&parse_legacy(&fs::read_to_string(&path)?)?);
                    atomic_write(
                        &target,
                        &serde_json::to_vec_pretty(&Envelope { version: 1, values })?,
                    )?;
                }
            }
        }
        for directory in ["profiles", "overrides"] {
            let source = preview.join(directory);
            if !source.try_exists()? {
                continue;
            }
            reject_link(&source)?;
            let destination = self.paths.data().join(directory);
            fs::create_dir_all(&destination)?;
            for entry in fs::read_dir(source)? {
                let entry = entry?;
                reject_link(&entry.path())?;
                if !entry.file_type()?.is_file() {
                    return Err(StorageError::UnsafeMigration(entry.path()));
                }
                let target = destination.join(entry.file_name());
                if !target.try_exists()? {
                    atomic_write(&target, &fs::read(entry.path())?)?;
                }
            }
        }
        atomic_write(&marker, b"1\n")
    }
}

pub(crate) fn contains_session_credentials(values: &Values) -> bool {
    SESSION_CREDENTIALS
        .iter()
        .any(|key| values.get(*key).is_some_and(|value| !value.is_empty()))
}
const SESSION_CREDENTIALS: [&str; 4] = [
    "Secret",
    "WebDavPassword",
    "GitHubToken",
    "GistAgeSecretKey",
];
fn persistent_values(values: &Values) -> Values {
    let mut values = values.clone();
    for key in SESSION_CREDENTIALS {
        values.remove(key);
    }
    values
}

fn reject_link(path: &Path) -> Result<(), StorageError> {
    if fs::symlink_metadata(path)?.file_type().is_symlink() {
        return Err(StorageError::UnsafeMigration(path.to_owned()));
    }
    Ok(())
}
fn decode(bytes: &[u8]) -> Result<Values, StorageError> {
    let envelope: Envelope = serde_json::from_slice(bytes)?;
    if envelope.version != 1 {
        return Err(StorageError::UnsupportedVersion(envelope.version));
    }
    Ok(envelope.values)
}
fn parse_legacy(text: &str) -> Result<Values, StorageError> {
    text.lines()
        .filter(|line| !line.is_empty())
        .map(|line| {
            let (key, value) = line.split_once('\t').ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidData, "预览配置缺少字段分隔符")
            })?;
            Ok((
                key.to_owned(),
                value
                    .replace("%09", "\t")
                    .replace("%0D", "\r")
                    .replace("%0A", "\n")
                    .replace("%25", "%"),
            ))
        })
        .collect()
}
pub(crate) fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), StorageError> {
    let directory = path
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "文件没有父目录"))?;
    fs::create_dir_all(directory)?;
    let mut temporary = tempfile::NamedTempFile::new_in(directory)?;
    temporary.write_all(bytes)?;
    temporary.as_file().sync_all()?;
    temporary.persist(path).map_err(|error| error.error)?;
    // Replacement is the commit point. A later directory flush failure must not
    // report an uncommitted write or make callers roll back only part of a save.
    #[cfg(unix)]
    if let Err(error) = fs::File::open(directory).and_then(|directory| directory.sync_all()) {
        eprintln!("文件已替换，但无法同步目录元数据：{error}");
    }
    Ok(())
}
