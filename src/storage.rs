//! Durable application storage. This module has no GPUI dependency.
mod paths;
#[cfg(test)]
mod tests;

pub(crate) use paths::{AppPaths, DirectoryKind};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs,
    io::{self, Read, Write},
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
    pub(crate) fn is_setting(&self) -> bool {
        matches!(
            self.name(),
            "settings" | "core" | "dns" | "sniffer" | "resources" | "sysproxy" | "tun"
        )
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
    #[error("{path}：{reason}；请修正文件后重试")]
    Document { path: PathBuf, reason: String },
    #[error("{0}")]
    Settings(String),
    #[error("存储操作失败：{0}")]
    Io(#[from] io::Error),
}

#[derive(Serialize, Deserialize)]
struct Envelope {
    version: u32,
    values: Values,
}

#[derive(Clone)]
pub(crate) struct Repository {
    paths: AppPaths,
}
impl Repository {
    pub(crate) fn open(paths: AppPaths) -> Result<Self, StorageError> {
        paths.create()?;
        Ok(Self { paths })
    }
    pub(crate) fn paths(&self) -> &AppPaths {
        &self.paths
    }
    fn path(&self, namespace: &Namespace) -> PathBuf {
        self.document_path(namespace)
    }
    pub(crate) fn document_path(&self, namespace: &Namespace) -> PathBuf {
        if namespace.name() == "profile-index"
            || (namespace.is_setting() && !matches!(namespace.name(), "dns" | "sniffer"))
        {
            return crate::preferences::app::path(&self.paths);
        }
        let directory = if namespace.is_setting() {
            self.paths.settings()
        } else {
            self.paths.indexes()
        };
        directory.join(format!(
            "{}.{}",
            namespace.name(),
            if matches!(namespace.name(), "dns" | "sniffer") {
                "yaml"
            } else {
                "json"
            }
        ))
    }
    pub(crate) fn read(&self, namespace: &Namespace) -> Result<Values, StorageError> {
        self.read_limited(namespace, crate::configuration::INPUT_LIMIT)
    }
    pub(crate) fn read_limited(
        &self,
        namespace: &Namespace,
        limit: u64,
    ) -> Result<Values, StorageError> {
        if namespace.name() == "profile-index" {
            return crate::profiles::settings::read_values(&self.paths, limit)
                .map_err(StorageError::Settings);
        }
        if namespace.name() == "tun" {
            return crate::configuration::Preferences::load(&self.paths)
                .and_then(|preferences| crate::configuration::tun::project(preferences.native()))
                .map_err(StorageError::Settings);
        }
        if namespace.is_setting() && !matches!(namespace.name(), "dns" | "sniffer") {
            return crate::preferences::app::read_values(
                &self.paths,
                crate::preferences::app::section(namespace.name()),
                limit,
            )
            .map_err(StorageError::Settings);
        }
        let file = match fs::File::open(self.path(namespace)) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Values::new()),
            Err(error) => return Err(error.into()),
        };
        let mut bytes = Vec::new();
        file.take(limit.saturating_add(1)).read_to_end(&mut bytes)?;
        if bytes.len() as u64 > limit {
            return Err(io::Error::other("配置存储超过读取上限").into());
        }
        let mut values =
            decode_document(&bytes, namespace).map_err(|error| StorageError::Document {
                path: self.path(namespace),
                reason: error.to_string(),
            })?;
        if let Some(kind) = crate::preferences::kind_for_page(namespace.name())
            .filter(|kind| crate::preferences::native::key(*kind).is_some())
        {
            let saved = crate::preferences::app::read_values(&self.paths, namespace.name(), limit)
                .map_err(StorageError::Settings)?;
            let flag = crate::preferences::native::flag(kind);
            if let Some(value) = saved.get(flag) {
                values.insert(flag.into(), value.clone());
            }
        }
        Ok(values)
    }
    pub(crate) fn snapshot(&self) -> Result<BTreeMap<Namespace, Values>, StorageError> {
        let mut snapshot = BTreeMap::new();
        for kind in crate::preferences::SettingsKind::ALL
            .into_iter()
            .filter(|kind| {
                !matches!(
                    kind,
                    crate::preferences::SettingsKind::Core
                        | crate::preferences::SettingsKind::Resources
                )
            })
        {
            let namespace = Namespace::parse(kind.id())?;
            let values = self.read(&namespace)?;
            if !values.is_empty() {
                snapshot.insert(namespace, values);
            }
        }
        let profiles = Namespace::parse("profile-index")?;
        snapshot.insert(profiles.clone(), self.read(&profiles)?);
        for entry in fs::read_dir(self.paths.indexes())? {
            let path = entry?.path();
            let Some(name) = path.file_stem().and_then(|name| name.to_str()) else {
                continue;
            };
            let Ok(namespace) = Namespace::parse(name) else {
                continue;
            };
            if !namespace.is_setting() && path == self.document_path(&namespace) {
                snapshot.insert(namespace.clone(), self.read(&namespace)?);
            }
        }
        Ok(snapshot)
    }
    pub(crate) fn available_snapshot(&self) -> BTreeMap<Namespace, Values> {
        let mut snapshot = BTreeMap::new();
        for kind in crate::preferences::SettingsKind::ALL
            .into_iter()
            .filter(|kind| {
                !matches!(
                    kind,
                    crate::preferences::SettingsKind::Core
                        | crate::preferences::SettingsKind::Resources
                )
            })
        {
            let Ok(namespace) = Namespace::parse(kind.id()) else {
                continue;
            };
            if let Ok(values) = self.read(&namespace)
                && !values.is_empty()
            {
                snapshot.insert(namespace, values);
            }
        }
        if let Ok(profiles) = Namespace::parse("profile-index")
            && let Ok(values) = self.read(&profiles)
        {
            snapshot.insert(profiles, values);
        }
        if let Ok(entries) = fs::read_dir(self.paths.indexes()) {
            for entry in entries.flatten() {
                let path = entry.path();
                let Some(name) = path.file_stem().and_then(|name| name.to_str()) else {
                    continue;
                };
                let Ok(namespace) = Namespace::parse(name) else {
                    continue;
                };
                if !namespace.is_setting()
                    && path == self.document_path(&namespace)
                    && let Ok(values) = self.read(&namespace)
                {
                    snapshot.insert(namespace, values);
                }
            }
        }
        snapshot
    }
    pub(crate) fn save(&self, namespace: &Namespace, values: &Values) -> Result<(), StorageError> {
        let values = persistent_values(values);
        if namespace.name() == "profile-index" {
            return crate::profiles::settings::write_values(&self.paths, &values)
                .map_err(StorageError::Settings);
        }
        if namespace.name() == "tun" {
            let previous = crate::configuration::Preferences::load(&self.paths)
                .map_err(StorageError::Settings)?;
            let mut combined = previous.values();
            combined.extend(values);
            return crate::configuration::Preferences::from_values(&combined, Some(&previous))
                .and_then(|next| next.save(&self.paths))
                .map_err(StorageError::Settings);
        }
        if namespace.is_setting() && !matches!(namespace.name(), "dns" | "sniffer") {
            return crate::preferences::app::write_values(
                &self.paths,
                crate::preferences::app::section(namespace.name()),
                &values,
            )
            .map_err(StorageError::Settings);
        }
        let bytes = if let Some(kind) = crate::preferences::kind_for_page(namespace.name())
            .filter(|kind| crate::preferences::native::key(*kind).is_some())
        {
            let bytes =
                crate::preferences::native::encode(kind, &values).map_err(io::Error::other)?;
            // Syntax/type errors must be detected before changing either file.
            let previous = fs::read(self.path(namespace)).ok();
            atomic_write(&self.path(namespace), &bytes)?;
            let flags = Values::from([(
                crate::preferences::native::flag(kind).into(),
                values
                    .get(crate::preferences::native::flag(kind))
                    .cloned()
                    .unwrap_or_else(|| "false".into()),
            )]);
            if let Err(error) =
                crate::preferences::app::write_values(&self.paths, namespace.name(), &flags)
            {
                let recovery = if let Some(previous) = previous {
                    atomic_write(&self.path(namespace), &previous)
                } else {
                    fs::remove_file(self.path(namespace)).map_err(StorageError::from)
                };
                return Err(StorageError::Settings(match recovery {
                    Ok(()) => error,
                    Err(recovery) => format!("{error}；YAML 恢复失败：{recovery}"),
                }));
            }
            return Ok(());
        } else if namespace.is_setting() {
            crate::preferences::json::encode(&values).map_err(io::Error::other)?
        } else {
            serde_json::to_vec_pretty(&Envelope { version: 1, values })?
        };
        atomic_write(&self.path(namespace), &bytes)
    }
}

#[cfg(test)]
pub(crate) fn contains_session_credentials(values: &Values) -> bool {
    SESSION_CREDENTIALS
        .iter()
        .any(|key| values.get(*key).is_some_and(|value| !value.is_empty()))
}
const SESSION_CREDENTIALS: [&str; 3] = ["Secret", "GitHubToken", "GistAgeSecretKey"];
fn persistent_values(values: &Values) -> Values {
    let mut values = values.clone();
    for key in SESSION_CREDENTIALS {
        values.remove(key);
    }
    values
}

fn decode_document(bytes: &[u8], namespace: &Namespace) -> Result<Values, StorageError> {
    if let Some(kind) = crate::preferences::kind_for_page(namespace.name())
        .filter(|kind| crate::preferences::native::key(*kind).is_some())
    {
        return crate::preferences::native::decode(kind, bytes)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error).into());
    }
    if !namespace.is_setting() {
        return decode(bytes);
    }
    crate::preferences::json::decode(bytes)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error).into())
}
fn decode(bytes: &[u8]) -> Result<Values, StorageError> {
    let envelope: Envelope = serde_json::from_slice(bytes)?;
    if envelope.version != 1 {
        return Err(StorageError::UnsupportedVersion(envelope.version));
    }
    Ok(envelope.values)
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
        log::warn!("文件已替换，但同步目录元数据失败（{error}）");
    }
    Ok(())
}
