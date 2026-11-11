//! Diagnostic copies preserve configuration structure and redact credentials before ZIP writes.
use crate::storage::{AppPaths, DirectoryKind};
use serde_yaml_ng::Value;
use std::{
    io::{Read as _, Write as _},
    path::{Path, PathBuf},
};

pub(super) fn export(
    zip: &mut zip::ZipWriter<&mut std::fs::File>,
    paths: &AppPaths,
) -> Result<Vec<String>, String> {
    let mut status = Vec::new();
    let mut files = Vec::new();
    for (area, root) in [
        ("settings", paths.settings()),
        ("indexes", paths.indexes()),
        ("profiles", paths.data().join("profiles")),
        ("overrides", paths.data().join("overrides")),
    ] {
        collect(&root, area, &mut files, &mut status, |name| match area {
            "settings" => matches!(
                name,
                "settings.json" | "mihomo.yaml" | "dns.yaml" | "sniffer.yaml"
            ),
            "indexes" => {
                name == "override-index.json"
                    || name
                        .strip_prefix("profile-enhance-")
                        .and_then(|name| name.strip_suffix(".json"))
                        .is_some_and(numeric_id)
            }
            _ => name.strip_suffix(".yaml").is_some_and(numeric_id),
        });
    }
    // These temporary directories belong to the application's current local launches.
    let core = paths.directory(DirectoryKind::Core);
    let entries = std::fs::symlink_metadata(&core).and_then(|meta| {
        if !meta.is_dir() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "来源不是普通目录",
            ));
        }
        std::fs::read_dir(&core)
    });
    match entries {
        Ok(entries) => {
            for entry in entries {
                match entry {
                    Ok(entry) => {
                        let name = entry.file_name().to_string_lossy().into_owned();
                        if name.starts_with("run-") {
                            collect(
                                &entry.path(),
                                &format!("runtime/{name}"),
                                &mut files,
                                &mut status,
                                |name| name == "candidate.yaml",
                            );
                        }
                    }
                    Err(error) => {
                        status.push(format!("runtime：目录条目读取失败，{}", error.kind()))
                    }
                }
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => status.push(format!("runtime：目录未导出，{}", error.kind())),
    }
    // Core settings come first if large subscriptions exhaust the shared budget.
    files.sort_by_key(|(name, _)| {
        let priority = match name.split('/').next() {
            Some("settings") => 0,
            Some("indexes") => 2,
            Some("profiles") => 3,
            Some("overrides") => 4,
            _ => 1,
        };
        (priority, name.clone())
    });
    let mut total = 0;
    for (name, path) in files {
        let result = read(&path, &mut total).and_then(|bytes| sanitize(&name, &bytes, paths));
        match result {
            Ok((content, note)) => {
                let target = format!(
                    "configuration/{name}{}",
                    if note.is_some() { ".redacted.txt" } else { "" }
                );
                write(zip, &target, &content)?;
                if let Some(note) = note {
                    status.push(format!("{name}：{note}"));
                }
            }
            Err(error) => status.push(format!("{name}：未导出，{error}")),
        }
    }
    Ok(status)
}

fn numeric_id(name: &str) -> bool {
    name.parse::<u64>().is_ok_and(|id| id > 0)
}

fn collect(
    root: &Path,
    area: &str,
    files: &mut Vec<(String, PathBuf)>,
    status: &mut Vec<String>,
    include: impl Fn(&str) -> bool,
) {
    let entries = (|| -> std::io::Result<_> {
        if !std::fs::symlink_metadata(root)?.is_dir() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "来源不是普通目录",
            ));
        }
        std::fs::read_dir(root)
    })();
    match entries {
        Ok(entries) => {
            for entry in entries {
                match entry {
                    Ok(entry) => {
                        let name = entry.file_name().to_string_lossy().into_owned();
                        if include(&name) {
                            files.push((format!("{area}/{name}"), entry.path()));
                        }
                    }
                    Err(error) => {
                        status.push(format!("{area}：目录条目读取失败，{}", error.kind()))
                    }
                }
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => status.push(format!("{area}：目录未导出，{}", error.kind())),
    }
}

fn read(path: &Path, total: &mut usize) -> Result<Vec<u8>, String> {
    const TOTAL: usize = 32 * 1024 * 1024;
    let metadata = std::fs::symlink_metadata(path).map_err(|error| error.kind().to_string())?;
    if !metadata.is_file() {
        return Err("来源不是普通文件".into());
    }
    let limit = (crate::configuration::INPUT_LIMIT as usize).min(TOTAL - *total);
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .map_err(|error| error.kind().to_string())?
        .take(limit as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| error.kind().to_string())?;
    *total += bytes.len().min(limit);
    if bytes.len() > limit {
        return Err("达到配置导出大小上限".into());
    }
    Ok(bytes)
}

pub(super) fn sanitize(
    name: &str,
    bytes: &[u8],
    paths: &AppPaths,
) -> Result<(String, Option<String>), String> {
    let parsed: Result<Value, String> = if name.ends_with(".json") {
        serde_json::from_slice::<serde_json::Value>(bytes)
            .map_err(|error| {
                format!(
                    "JSON 格式无效，第 {} 行第 {} 列",
                    error.line(),
                    error.column()
                )
            })
            .and_then(|value| {
                serde_yaml_ng::to_value(value).map_err(|_| "无法读取 JSON 结构".into())
            })
    } else {
        serde_yaml_ng::from_slice(bytes).map_err(|error| {
            error.location().map_or_else(
                || "YAML 格式无效".into(),
                |location| {
                    format!(
                        "YAML 格式无效，第 {} 行第 {} 列",
                        location.line(),
                        location.column()
                    )
                },
            )
        })
    };
    match parsed {
        Ok(mut value) => {
            // Enhancement fragments are YAML strings in the current index JSON format.
            if name.starts_with("indexes/profile-enhance-")
                && let Some(values) = value.get_mut("values").and_then(Value::as_mapping_mut)
            {
                for key in ["proxies", "proxy-groups", "rules"] {
                    if let Some(Value::String(text)) = values.get_mut(Value::String(key.into())) {
                        *text = match serde_yaml_ng::from_str::<Value>(text) {
                            Ok(mut fragment) => {
                                redact_value(&mut fragment, false, paths);
                                serde_yaml_ng::to_string(&fragment)
                                    .map_err(|_| "无法编码脱敏增强片段")?
                            }
                            Err(_) => "<redacted: invalid YAML>".into(),
                        };
                    }
                }
            }
            redact_value(&mut value, false, paths);
            let content = if name.ends_with(".json") {
                serde_json::to_string_pretty(&value)
            } else {
                return serde_yaml_ng::to_string(&value)
                    .map(|text| (text, None))
                    .map_err(|_| "无法编码脱敏 YAML".into());
            };
            content
                .map(|text| (text, None))
                .map_err(|_| "无法编码脱敏 JSON".into())
        }
        Err(error) => {
            let content = String::from_utf8_lossy(bytes)
                .chars()
                .map(|ch| {
                    if ch.is_whitespace() || "{}[]:,\"'-".contains(ch) {
                        ch
                    } else {
                        '*'
                    }
                })
                .collect();
            Ok((content, Some(format!("{error}，已导出结构脱敏副本"))))
        }
    }
}

fn sensitive(key: &str) -> bool {
    let name = key
        .rsplit('/')
        .next()
        .unwrap_or(key)
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .collect::<String>()
        .to_ascii_lowercase();
    matches!(
        name.as_str(),
        "user"
            | "username"
            | "uuid"
            | "authentication"
            | "auth"
            | "authstr"
            | "headers"
            | "header"
            | "server"
            | "content"
            | "recipient"
            | "shortid"
    ) || [
        "password",
        "passwd",
        "passphrase",
        "token",
        "secret",
        "key",
        "credentials",
        "authorization",
        "cookie",
        "script",
    ]
    .iter()
    .any(|suffix| name.ends_with(suffix))
}

fn redact_value(value: &mut Value, hidden: bool, paths: &AppPaths) {
    match value {
        Value::Mapping(mapping) => {
            for (mut key, mut value) in std::mem::take(mapping) {
                let hide = hidden || key.as_str().is_some_and(sensitive);
                redact_value(&mut key, false, paths);
                redact_value(&mut value, hide, paths);
                mapping.insert(key, value);
            }
        }
        Value::Sequence(items) => {
            for value in items {
                redact_value(value, hidden, paths);
            }
        }
        Value::Tagged(tagged) => redact_value(&mut tagged.value, hidden, paths),
        Value::String(text) if !text.is_empty() => {
            *text = if hidden {
                "<redacted>".into()
            } else {
                super::redact(text, paths)
            }
        }
        Value::Number(_) if hidden => *value = Value::String("<redacted>".into()),
        _ => {}
    }
}

fn write(
    zip: &mut zip::ZipWriter<&mut std::fs::File>,
    name: &str,
    text: &str,
) -> Result<(), String> {
    zip.start_file(name, zip::write::SimpleFileOptions::default())
        .map_err(|error| crate::logging::diagnostic_error("无法写入诊断配置", error))?;
    zip.write_all(text.as_bytes())
        .map_err(|error| crate::logging::diagnostic_error("无法写入诊断配置".to_owned(), error))
}
