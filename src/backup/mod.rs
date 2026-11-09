//! Complete user-configuration archives. The lifecycle actor owns capture and restoration.
mod gist;
pub(crate) mod local;
use crate::storage::{AppPaths, Namespace, Repository, atomic_write};
use base64::Engine as _;
pub(crate) use gist::Gist;
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};
use std::{
    collections::BTreeMap,
    fs,
    io::{Cursor, Read as _, Write as _},
    path::{Component, Path, PathBuf},
};
const TOTAL_LIMIT: usize = 32 * 1024 * 1024;
pub(crate) const ENCODED_LIMIT: usize = 48 * 1024 * 1024;
const HEADER: &str = "SUKI-CLASH-BACKUP-1\n";
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    version: u32,
    created_at: String,
    files: BTreeMap<String, String>,
}
#[derive(Clone)]
pub(crate) struct Archive {
    files: BTreeMap<String, Vec<u8>>,
}
pub(crate) struct Staged {
    pub(crate) paths: AppPaths,
    _directory: tempfile::TempDir,
}
pub(crate) struct Restore {
    previous: BTreeMap<String, Vec<u8>>,
}
fn digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}
fn managed(name: &str) -> bool {
    let parts = name.split('/').collect::<Vec<_>>();
    if parts.len() != 2
        || Path::new(name)
            .components()
            .any(|part| !matches!(part, Component::Normal(_)))
    {
        return false;
    }
    match parts[0] {
        "settings" => matches!(
            parts[1],
            "settings.json" | "mihomo.yaml" | "dns.yaml" | "sniffer.yaml"
        ),
        "indexes" => {
            parts[1] == "override-index.json"
                || parts[1]
                    .strip_prefix("profile-enhance-")
                    .and_then(|name| name.strip_suffix(".json"))
                    .is_some_and(|id| id.parse::<u64>().is_ok())
        }
        "profiles" => parts[1]
            .strip_suffix(".yaml")
            .is_some_and(|id| id.parse::<u64>().is_ok_and(|id| id > 0)),
        "overrides" => [".yaml", ".js"].iter().any(|ext| {
            parts[1]
                .strip_suffix(ext)
                .is_some_and(|id| id.parse::<u64>().is_ok_and(|id| id > 0))
        }),
        _ => false,
    }
}
fn target(paths: &AppPaths, name: &str) -> PathBuf {
    let (directory, file) = name.split_once('/').unwrap_or(("", ""));
    if directory == "settings" {
        paths.settings().join(file)
    } else {
        paths.data().join(directory).join(file)
    }
}
fn sync_key(name: &str) -> bool {
    let name = name
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .collect::<String>()
        .to_ascii_lowercase();
    name.contains("gist") || name.contains("github") || name.contains("sync") || name == "autorun"
}
fn sanitized_settings(bytes: &[u8], local: Option<&[u8]>) -> Result<Vec<u8>, String> {
    let mut root: serde_json::Map<String, serde_json::Value> = serde_json::from_slice(bytes)
        .map_err(|error| crate::logging::rejected("备份程序设置格式无效", error))?;
    if root
        .get("schema_version")
        .and_then(serde_json::Value::as_u64)
        != Some(1)
    {
        return Err("备份程序设置版本无效".into());
    }
    root.remove("backup");
    let local = local
        .map(serde_json::from_slice::<serde_json::Value>)
        .transpose()
        .map_err(|error| crate::logging::rejected("本机程序设置格式无效，不能恢复覆盖", error))?;
    let general = root
        .entry("general")
        .or_insert_with(|| serde_json::json!({}))
        .as_object_mut()
        .ok_or("备份常规设置格式无效")?;
    general.retain(|key, _| !sync_key(key));
    if let Some(local) = &local
        && let Some(current) = local.get("general").and_then(serde_json::Value::as_object)
    {
        for (key, value) in current.iter().filter(|(key, _)| sync_key(key)) {
            general.insert(key.clone(), value.clone());
        }
    }
    if let Some(local) = &local
        && let Some(backup) = local.get("backup")
    {
        root.insert("backup".into(), backup.clone());
    }
    // The independent secure credential store is deliberately not an archive member.
    serde_json::to_vec_pretty(&root)
        .map_err(|error| crate::logging::diagnostic_error("无法编码备份设置".into(), error))
}
fn collect(paths: &AppPaths) -> Result<BTreeMap<String, Vec<u8>>, String> {
    let mut files = BTreeMap::new();
    let mut total = 0usize;
    for directory in ["settings", "indexes", "profiles", "overrides"] {
        let root = if directory == "settings" {
            paths.settings()
        } else {
            paths.data().join(directory)
        };
        match fs::symlink_metadata(&root) {
            Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
                return Err(format!("{}：备份来源目录类型无效", root.display()));
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => {
                return Err(crate::logging::diagnostic_error(
                    format!("{}：无法读取备份目录", root.display()),
                    error,
                ));
            }
        }
        let entries = match fs::read_dir(&root) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => {
                return Err(crate::logging::diagnostic_error(
                    format!("{}：无法读取备份来源", root.display()),
                    error,
                ));
            }
        };
        for entry in entries {
            let entry = entry
                .map_err(|error| crate::logging::diagnostic_error("无法读取备份目录条目", error))?;
            let name = format!(
                "{directory}/{}",
                entry.file_name().to_str().ok_or("备份文件名不是UTF-8")?
            );
            if !managed(&name) {
                continue;
            }
            let metadata = entry
                .file_type()
                .map_err(|error| crate::logging::diagnostic_error("无法读取备份文件类型", error))?;
            if !metadata.is_file() || metadata.is_symlink() {
                return Err(format!("{name}：备份只接受普通文件"));
            }
            let file = fs::File::open(entry.path()).map_err(|error| {
                crate::logging::diagnostic_error(format!("{name}：无法读取备份文件"), error)
            })?;
            let mut bytes = Vec::new();
            file.take((TOTAL_LIMIT + 1) as u64)
                .read_to_end(&mut bytes)
                .map_err(|error| {
                    crate::logging::diagnostic_error(format!("{name}：无法读取备份文件"), error)
                })?;
            total = total.checked_add(bytes.len()).ok_or("备份超过大小上限")?;
            if total > TOTAL_LIMIT {
                return Err("备份超过32MiB上限".into());
            }
            files.insert(name, bytes);
        }
    }
    Ok(files)
}
impl Archive {
    pub(crate) fn capture(paths: &AppPaths) -> Result<Self, String> {
        let mut files = collect(paths)?;
        if let Some(bytes) = files.get_mut("settings/settings.json") {
            *bytes = sanitized_settings(bytes, None)?;
        }
        if !files.contains_key("settings/settings.json") {
            return Err("缺少程序设置，无法创建完整备份".into());
        }
        let archive = Self { files };
        let _ = archive.stage(paths)?;
        Ok(archive)
    }
    pub(crate) fn encode(&self, recipient: Option<&str>) -> Result<String, String> {
        let manifest = Manifest {
            version: 1,
            created_at: chrono::Utc::now().to_rfc3339(),
            files: self
                .files
                .iter()
                .map(|(name, bytes)| (name.clone(), digest(bytes)))
                .collect(),
        };
        let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        writer
            .start_file("manifest.json", options)
            .map_err(|error| crate::logging::diagnostic_error("无法创建备份清单", error))?;
        writer
            .write_all(
                &serde_json::to_vec(&manifest)
                    .map_err(|error| crate::logging::diagnostic_error("无法编码备份清单", error))?,
            )
            .map_err(|error| crate::logging::diagnostic_error("无法写入备份清单", error))?;
        for (name, bytes) in &self.files {
            writer
                .start_file(name, options)
                .map_err(|error| crate::logging::diagnostic_error("无法创建备份条目", error))?;
            writer
                .write_all(bytes)
                .map_err(|error| crate::logging::diagnostic_error("无法写入备份条目", error))?;
        }
        let bytes = writer
            .finish()
            .map_err(|error| crate::logging::diagnostic_error("无法完成备份压缩", error))?
            .into_inner();
        let text = format!(
            "{HEADER}{}",
            base64::engine::general_purpose::STANDARD.encode(bytes)
        );
        let text = if let Some(recipient) = recipient {
            let recipient = recipient
                .parse::<age::x25519::Recipient>()
                .map_err(|error| crate::logging::rejected("Age接收方公钥无效", error))?;
            let encryptor =
                age::Encryptor::with_recipients(std::iter::once(&recipient as &dyn age::Recipient))
                    .map_err(|error| {
                        crate::logging::diagnostic_error("无法建立Age备份加密", error)
                    })?;
            let mut encrypted = Vec::new();
            let armor = age::armor::ArmoredWriter::wrap_output(
                &mut encrypted,
                age::armor::Format::AsciiArmor,
            )
            .map_err(|error| crate::logging::diagnostic_error("无法创建Age备份", error))?;
            let mut output = encryptor
                .wrap_output(armor)
                .map_err(|error| crate::logging::diagnostic_error("无法创建Age备份", error))?;
            output
                .write_all(text.as_bytes())
                .map_err(|error| crate::logging::diagnostic_error("无法加密备份", error))?;
            output
                .finish()
                .and_then(|armor| armor.finish())
                .map_err(|error| crate::logging::diagnostic_error("无法完成Age备份", error))?;
            String::from_utf8(encrypted)
                .map_err(|error| crate::logging::rejected("Age备份编码无效", error))?
        } else {
            text
        };
        if text.len() > ENCODED_LIMIT {
            return Err("备份编码超过48MiB上限".into());
        }
        Ok(text)
    }
    pub(crate) fn decode(text: &str, identity: Option<&str>) -> Result<Self, String> {
        if text.len() > ENCODED_LIMIT {
            return Err("备份超过48MiB上限".into());
        }
        let decrypted;
        let text = if text.starts_with("-----BEGIN AGE ENCRYPTED FILE-----") {
            let identity = identity
                .ok_or("此备份需要Age私钥")?
                .parse::<age::x25519::Identity>()
                .map_err(|error| crate::logging::rejected("Age私钥无效", error))?;
            let decryptor = age::Decryptor::new(age::armor::ArmoredReader::new(text.as_bytes()))
                .map_err(|error| crate::logging::rejected("Age备份格式无效", error))?;
            let reader = decryptor
                .decrypt(std::iter::once(&identity as &dyn age::Identity))
                .map_err(|error| crate::logging::diagnostic_error("Age备份解密失败", error))?;
            let mut bytes = Vec::new();
            reader
                .take((ENCODED_LIMIT + 1) as u64)
                .read_to_end(&mut bytes)
                .map_err(|error| crate::logging::diagnostic_error("无法读取Age备份", error))?;
            if bytes.len() > ENCODED_LIMIT {
                return Err("Age备份解密后超过上限".into());
            }
            decrypted = String::from_utf8(bytes)
                .map_err(|error| crate::logging::rejected("备份不是UTF-8文本", error))?;
            decrypted.as_str()
        } else {
            text
        };
        let encoded = text
            .strip_prefix(HEADER)
            .ok_or("不是当前Suki Clash备份格式")?;
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .map_err(|error| crate::logging::rejected("备份编码无效", error))?;
        let mut zip = zip::ZipArchive::new(Cursor::new(bytes))
            .map_err(|error| crate::logging::rejected("备份压缩包无效", error))?;
        if zip.len() > 10001 {
            return Err("备份文件数量超过上限".into());
        }
        let mut files = BTreeMap::new();
        let mut manifest = None;
        let mut total = 0usize;
        for index in 0..zip.len() {
            let mut entry = zip
                .by_index(index)
                .map_err(|error| crate::logging::diagnostic_error("无法读取备份条目", error))?;
            let name = entry
                .name()
                .map_err(|error| crate::logging::rejected("备份文件名编码无效", error))?
                .into_owned();
            if entry.is_dir()
                || entry
                    .unix_mode()
                    .is_some_and(|mode| mode & 0o170000 == 0o120000)
                || (name != "manifest.json" && !managed(&name))
            {
                return Err("备份包含无效路径或文件类型".into());
            }
            if entry.size() > TOTAL_LIMIT as u64 {
                return Err("备份条目超过上限".into());
            }
            let mut bytes = Vec::new();
            (&mut entry)
                .take((TOTAL_LIMIT + 1) as u64)
                .read_to_end(&mut bytes)
                .map_err(|error| crate::logging::diagnostic_error("无法解压备份条目", error))?;
            total = total.checked_add(bytes.len()).ok_or("备份超过上限")?;
            if total > TOTAL_LIMIT {
                return Err("解压备份超过32MiB上限".into());
            }
            if name == "manifest.json" {
                if manifest.is_some() {
                    return Err("备份清单重复".into());
                }
                manifest = Some(
                    serde_json::from_slice::<Manifest>(&bytes)
                        .map_err(|error| crate::logging::rejected("备份清单格式无效", error))?,
                );
            } else if files.insert(name, bytes).is_some() {
                return Err("备份文件名重复".into());
            }
        }
        let manifest = manifest.ok_or("备份缺少清单")?;
        if manifest.version != 1
            || manifest.files.len() != files.len()
            || files
                .iter()
                .any(|(name, bytes)| manifest.files.get(name) != Some(&digest(bytes)))
        {
            return Err("备份版本或内容校验无效".into());
        }
        let bytes = files
            .get_mut("settings/settings.json")
            .ok_or("备份缺少程序设置")?;
        *bytes = sanitized_settings(bytes, None)?;
        Ok(Self { files })
    }
    pub(crate) fn stage(&self, live: &AppPaths) -> Result<Staged, String> {
        let directory = tempfile::tempdir()
            .map_err(|error| crate::logging::diagnostic_error("无法建立恢复预检目录", error))?;
        let paths = AppPaths::staged(directory.path());
        paths.create().map_err(crate::logging::error_message)?;
        let local_settings = match fs::read(crate::preferences::app::path(live)) {
            Ok(bytes) => Some(bytes),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => {
                return Err(crate::logging::diagnostic_error(
                    "无法读取本机设置，恢复未开始".into(),
                    error,
                ));
            }
        };
        for (name, bytes) in &self.files {
            let bytes = if name == "settings/settings.json" {
                sanitized_settings(bytes, local_settings.as_deref())?
            } else {
                bytes.clone()
            };
            let path = target(&paths, name);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)
                    .map_err(|error| crate::logging::diagnostic_error("无法创建恢复目录", error))?;
            }
            atomic_write(&path, &bytes).map_err(crate::logging::error_message)?;
        }
        let repository = Repository::open(paths.clone()).map_err(crate::logging::error_message)?;
        for kind in crate::preferences::SettingsKind::ALL {
            let values = crate::preferences::values(&repository, kind)
                .map_err(crate::logging::error_message)?;
            crate::preferences::validate(kind, &values)?;
        }
        crate::configuration::Preferences::load(&paths)?;
        let (_, profiles) = crate::profiles::repository::read(&paths)?;
        crate::profiles::override_repository::read(&paths)?;
        if let Some(id) = crate::configuration::selected_profile(&paths)?
            && !profiles.iter().any(|doc| doc.id == id)
        {
            return Err("备份选中订阅不存在".into());
        }
        crate::proxies::chain_config::Config::load(&paths)?;
        crate::proxies::runtime::Selections::load(&paths)?;
        for name in self
            .files
            .keys()
            .filter(|name| name.starts_with("indexes/profile-enhance-"))
        {
            let namespace = name
                .strip_prefix("indexes/")
                .and_then(|name| name.strip_suffix(".json"))
                .ok_or("增强备份路径无效")?;
            let values = repository
                .read(&Namespace::parse(namespace).map_err(crate::logging::error_message)?)
                .map_err(crate::logging::error_message)?;
            crate::configuration::enhancement::apply(b"{}", &values)?;
        }
        Ok(Staged {
            paths,
            _directory: directory,
        })
    }
    pub(crate) fn apply(&self, paths: &AppPaths) -> Result<Restore, String> {
        let _ = self.stage(paths)?;
        let previous = collect(paths)?;
        let mut next = self.files.clone();
        if let Some(bytes) = next.get_mut("settings/settings.json") {
            *bytes = sanitized_settings(
                bytes,
                previous.get("settings/settings.json").map(Vec::as_slice),
            )?;
        }
        if let Err(cause) = replace(paths, &next) {
            return Err(match replace(paths, &previous) {
                Ok(()) => cause,
                Err(recovery) => format!("{cause}；恢复原配置失败：{recovery}"),
            });
        }
        Ok(Restore { previous })
    }
}
fn replace(paths: &AppPaths, files: &BTreeMap<String, Vec<u8>>) -> Result<(), String> {
    let current = collect(paths)?;
    for (name, bytes) in files {
        let path = target(paths, name);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .map_err(|error| crate::logging::diagnostic_error("无法创建恢复目录", error))?;
        }
        atomic_write(&path, bytes).map_err(crate::logging::error_message)?;
    }
    for name in current.keys().filter(|name| !files.contains_key(*name)) {
        fs::remove_file(target(paths, name)).map_err(|error| {
            crate::logging::diagnostic_error(format!("{name}：无法清理本次恢复替换的配置"), error)
        })?;
    }
    Ok(())
}
impl Restore {
    pub(crate) fn rollback(self, paths: &AppPaths) -> Result<(), String> {
        replace(paths, &self.previous)
    }
    pub(crate) fn finish(self) {}
}

#[cfg(test)]
mod tests {
    use super::*;
    fn paths(root: &Path) -> AppPaths {
        let paths = AppPaths::staged(root);
        let repository = Repository::open(paths.clone()).unwrap();
        crate::preferences::initialize(&repository).unwrap();
        crate::configuration::initialize_selection(&paths).unwrap();
        paths
    }
    #[test]
    fn full_archive_roundtrip_preserves_native_documents_and_local_sync_settings() {
        let root = tempfile::tempdir().unwrap();
        let source = paths(&root.path().join("source"));
        crate::preferences::app::write(
            &source,
            "backup",
            &serde_json::json!({"encrypt":true,"recipient":"private-sync"}),
        )
        .unwrap();
        let mut general =
            crate::preferences::app::read_values(&source, "general", 8 * 1024 * 1024).unwrap();
        general.insert("GitHubToken".into(), "never-backup".into());
        general.insert("AutoRun".into(), "true".into());
        crate::preferences::app::write_values(&source, "general", &general).unwrap();
        let document =
            crate::profiles::model::Document::new(7, crate::profiles::model::DocumentKind::Profile);
        crate::profiles::repository::commit(&source, &[], &[document], Some(7)).unwrap();
        let mut selections = crate::proxies::runtime::Selections::load(&source).unwrap();
        selections.update(Some(7), "group", Some("node"));
        selections.save(&source).unwrap();
        atomic_write(&source.traffic_record(), b"cache excluded from backup").unwrap();
        let archive = Archive::capture(&source).unwrap();
        let settings = std::str::from_utf8(&archive.files["settings/settings.json"]).unwrap();
        assert!(!settings.contains("never-backup"));
        assert!(!settings.contains("private-sync"));
        let text = archive.encode(None).unwrap();
        let decoded = Archive::decode(&text, None).unwrap();
        assert_eq!(decoded.files, archive.files);
        let destination = paths(&root.path().join("destination"));
        crate::preferences::app::write(
            &destination,
            "backup",
            &serde_json::json!({"encrypt":false,"recipient":"keep-local"}),
        )
        .unwrap();
        let before = collect(&destination).unwrap();
        let restore = decoded.apply(&destination).unwrap();
        let settings = fs::read_to_string(crate::preferences::app::path(&destination)).unwrap();
        assert!(settings.contains("keep-local"));
        assert!(!settings.contains("private-sync"));
        assert_eq!(
            crate::configuration::selected_profile(&destination).unwrap(),
            Some(7)
        );
        assert_eq!(
            crate::profiles::repository::read(&destination).unwrap().1[0].id,
            7
        );
        assert_eq!(
            crate::proxies::runtime::Selections::load(&destination)
                .unwrap()
                .for_profile(Some(7))["group"],
            "node"
        );
        assert!(!destination.traffic_record().exists());
        assert!(archive.files.keys().all(|name| !name.starts_with("cache/")));

        assert!(
            !crate::preferences::app::read_values(&destination, "general", 8 * 1024 * 1024)
                .unwrap()
                .get("AutoRun")
                .is_some_and(|value| value == "true")
        );
        restore.rollback(&destination).unwrap();
        assert_eq!(collect(&destination).unwrap(), before);
    }
    #[test]
    fn encrypted_archive_requires_matching_identity_and_checks_content_digest() {
        use age::secrecy::ExposeSecret as _;
        let root = tempfile::tempdir().unwrap();
        let source = paths(root.path());
        let archive = Archive::capture(&source).unwrap();
        let key = age::x25519::Identity::generate();
        let public = key.to_public().to_string();
        let encrypted = archive.encode(Some(&public)).unwrap();
        assert!(encrypted.starts_with("-----BEGIN AGE ENCRYPTED FILE-----"));
        assert!(Archive::decode(&encrypted, None).is_err());
        let wrong = age::x25519::Identity::generate().to_string();
        assert!(Archive::decode(&encrypted, Some(wrong.expose_secret())).is_err());
        let key = key.to_string();
        assert_eq!(
            Archive::decode(&encrypted, Some(key.expose_secret()))
                .unwrap()
                .files,
            archive.files
        );
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        zip.start_file("manifest.json", zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(br#"{"version":1,"created_at":"now","files":{"profiles/1.yaml":"wrong"}}"#)
            .unwrap();
        zip.start_file("profiles/1.yaml", zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(b"proxies: []").unwrap();
        let bytes = zip.finish().unwrap().into_inner();
        assert!(
            Archive::decode(
                &format!(
                    "{HEADER}{}",
                    base64::engine::general_purpose::STANDARD.encode(bytes)
                ),
                None
            )
            .is_err()
        );
    }
    #[test]
    fn archive_paths_are_allowlisted_and_missing_profile_body_fails_before_publish() {
        for name in [
            "../settings.json",
            "settings/../x",
            "profiles/../../x",
            "settings/credentials.json",
            "indexes/system-proxy.json",
            "profiles/1.yaml/x",
            "core/mihomo",
        ] {
            assert!(!managed(name), "{name}");
        }
        let root = tempfile::tempdir().unwrap();
        let source = paths(root.path());
        let namespace = Namespace::parse("profile-index").unwrap();
        let repository = Repository::open(source.clone()).unwrap();
        let values = crate::storage::Values::from([
            ("format".into(), "1".into()),
            ("ids".into(), "1".into()),
            ("1/kind".into(), "配置".into()),
            ("1/name".into(), "missing".into()),
        ]);
        repository.save(&namespace, &values).unwrap();
        assert!(Archive::capture(&source).is_err());
    }
}
