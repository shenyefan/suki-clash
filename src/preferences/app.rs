//! One application settings document. Section updates serialize across background owners.
use crate::storage::{AppPaths, Values, atomic_write};
use serde::{Serialize, de::DeserializeOwned};
use serde_json::{Map, Value};
use std::{io::Read as _, path::PathBuf};
// Production writes run on the lifecycle actor; startup initialization completes
// before that actor is installed. Keep one owner instead of a second lock registry.
const LIMIT: u64 = 8 * 1024 * 1024;
pub(crate) fn path(paths: &AppPaths) -> PathBuf {
    paths.settings().join("settings.json")
}
fn document(paths: &AppPaths, limit: u64) -> Result<Map<String, Value>, String> {
    let path = path(paths);
    let file = match std::fs::File::open(&path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(Map::from_iter([("schema_version".into(), 1.into())]));
        }
        Err(_) => return Err(format!("{}：无法读取程序设置", path.display())),
    };
    let mut bytes = Vec::new();
    file.take(limit.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|_| format!("{}：无法读取程序设置", path.display()))?;
    if bytes.len() as u64 > limit {
        return Err(format!("{}：程序设置超过读取上限", path.display()));
    }
    let root: Map<String, Value> = serde_json::from_slice(&bytes).map_err(|error| {
        format!(
            "{}：JSON 格式或类型无效，第 {} 行第 {} 列；请修正文件后重试",
            path.display(),
            error.line(),
            error.column()
        )
    })?;
    if root.get("schema_version").and_then(Value::as_u64) != Some(1) {
        return Err(format!("{}：程序设置版本无效", path.display()));
    }
    Ok(root)
}
pub(crate) fn read<T: DeserializeOwned + Default>(
    paths: &AppPaths,
    section: &str,
) -> Result<T, String> {
    let root = document(paths, LIMIT)?;
    match root.get(section) {
        Some(value) => serde_json::from_value(value.clone())
            .map_err(|_| format!("{}：{section} 设置类型无效", path(paths).display())),
        None => Ok(T::default()),
    }
}
pub(crate) fn write<T: Serialize>(
    paths: &AppPaths,
    section: &str,
    value: &T,
) -> Result<(), String> {
    let value = serde_json::to_value(value).map_err(|_| "无法编码程序设置")?;
    let mut root = document(paths, LIMIT)?;
    root.insert(section.into(), value);
    atomic_write(
        &path(paths),
        &serde_json::to_vec_pretty(&root).map_err(|_| "无法编码程序设置")?,
    )
    .map_err(|error| error.to_string())
}
pub(crate) fn read_values(paths: &AppPaths, section: &str, limit: u64) -> Result<Values, String> {
    let root = document(paths, limit)?;
    let Some(section) = root.get(section) else {
        return Ok(Values::new());
    };
    let mut section = section
        .as_object()
        .cloned()
        .ok_or_else(|| format!("{}：程序设置分组需为对象", path(paths).display()))?;
    section.insert("schema_version".into(), 2.into());
    super::json::decode(&serde_json::to_vec(&section).map_err(|_| "无法读取程序设置")?)
        .map_err(|error| format!("{}：{error}", path(paths).display()))
}
pub(crate) fn write_values(paths: &AppPaths, section: &str, values: &Values) -> Result<(), String> {
    let mut value: Map<String, Value> =
        serde_json::from_slice(&super::json::encode(values)?).map_err(|_| "无法编码程序设置")?;
    value.remove("schema_version");
    write(paths, section, &value)
}
pub(crate) fn section(namespace: &str) -> &str {
    if namespace == "settings" {
        "general"
    } else {
        namespace
    }
}

/// A transaction restores only the sections it owns, preserving other writers.
#[derive(serde::Serialize, serde::Deserialize)]
pub(crate) struct Sections(std::collections::BTreeMap<String, Option<Value>>);
pub(crate) fn capture(paths: &AppPaths, sections: &[&str]) -> Result<Sections, String> {
    let root = document(paths, LIMIT)?;
    Ok(Sections(
        sections
            .iter()
            .map(|name| ((*name).to_owned(), root.get(*name).cloned()))
            .collect(),
    ))
}
pub(crate) fn restore(paths: &AppPaths, snapshot: &Sections) -> Result<(), String> {
    let mut root = document(paths, LIMIT)?;
    for (section, value) in &snapshot.0 {
        match value {
            Some(value) => {
                root.insert(section.clone(), value.clone());
            }
            None => {
                root.remove(section);
            }
        }
    }
    atomic_write(
        &path(paths),
        &serde_json::to_vec_pretty(&root).map_err(|_| "无法编码程序设置")?,
    )
    .map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn section_updates_and_scoped_recovery_preserve_other_settings() {
        let root = tempfile::tempdir().unwrap();
        let paths = AppPaths::isolated(root.path());
        paths.create().unwrap();
        for (section, key) in [
            ("general", "ThemeIndex"),
            ("core", "EnableExternalController"),
            ("dns", "DnsOverrideEnabled"),
            ("sniffer", "SnifferOverrideEnabled"),
        ] {
            write_values(
                &paths,
                section,
                &Values::from([(
                    key.into(),
                    if key == "ThemeIndex" { "dark" } else { "true" }.into(),
                )]),
            )
            .unwrap();
        }
        let before = capture(&paths, &["runtime", "core"]).unwrap();
        write_values(
            &paths,
            "general",
            &Values::from([("ThemeIndex".into(), "light".into())]),
        )
        .unwrap();
        write(
            &paths,
            "runtime",
            &serde_json::json!({"selected_profile_id": 7}),
        )
        .unwrap();
        restore(&paths, &before).unwrap();
        let root = document(&paths, LIMIT).unwrap();
        assert_eq!(root["general"]["theme_index"], "light");
        assert_eq!(root["core"]["enable_external_controller"], true);
        assert_eq!(root["dns"]["dns_override_enabled"], true);
        assert_eq!(root["sniffer"]["sniffer_override_enabled"], true);
        assert!(!root.contains_key("runtime"));
        assert_eq!(std::fs::read_dir(paths.settings()).unwrap().count(), 1);
    }
    #[test]
    fn bad_sections_are_isolated_and_invalid_json_is_never_replaced() {
        let root = tempfile::tempdir().unwrap();
        let paths = AppPaths::isolated(root.path());
        paths.create().unwrap();
        std::fs::write(path(&paths), br#"{"schema_version":1,"general":{"app_log_level":false},"dns":{"dns_override_enabled":true}}"#).unwrap();
        assert!(read_values(&paths, "general", LIMIT).is_err());
        assert_eq!(
            read_values(&paths, "dns", LIMIT).unwrap()["DnsOverrideEnabled"],
            "true"
        );
        write(
            &paths,
            "runtime",
            &serde_json::json!({"selected_profile_id":7}),
        )
        .unwrap();
        assert!(read_values(&paths, "general", LIMIT).is_err());
        std::fs::write(path(&paths), b"{\n broken").unwrap();
        let before = std::fs::read(path(&paths)).unwrap();
        assert!(
            write_values(&paths, "dns", &Values::new())
                .unwrap_err()
                .contains("第 2 行")
        );
        assert_eq!(std::fs::read(path(&paths)).unwrap(), before);
    }
}
