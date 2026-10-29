//! Document metadata is separate from UI entities; subscription secrets use the credential store.
use super::model::{Document, DocumentKind, TrafficUsage};
use crate::integration as storage;
use std::{collections::BTreeMap, fs, path::PathBuf};

pub(super) fn key(overrides: bool) -> &'static str {
    if overrides {
        "override-index"
    } else {
        "profile-index"
    }
}
pub(super) fn load(overrides: bool, cx: &gpui_kit::App) -> Option<(Vec<Document>, Option<u64>)> {
    load_values(storage::read(key(overrides), cx))
}
pub(crate) fn load_values(
    values: BTreeMap<String, String>,
) -> Option<(Vec<Document>, Option<u64>)> {
    if values.get("format").map(String::as_str) != Some("1") {
        return None;
    }
    let ids = values
        .get("ids")?
        .split(',')
        .filter_map(|id| id.parse::<u64>().ok());
    let documents = ids
        .map(|id| {
            let get = |field: &str| {
                values
                    .get(&format!("{id}/{field}"))
                    .cloned()
                    .unwrap_or_default()
            };
            let kind = match get("kind").as_str() {
                "JavaScript" => DocumentKind::JavaScript,
                "YAML" => DocumentKind::Yaml,
                _ => DocumentKind::Profile,
            };
            let mut doc = Document::new(id, kind);
            doc.name = get("name");
            doc.url = get("url");
            doc.content = get("content");
            doc.enabled = get("enabled") == "true";
            doc.scope_profile = get("scope-profile").parse().ok();
            doc.scope_name = get("scope-name");
            doc.interval = get("interval");
            doc.timeout = get("timeout");
            doc.user_agent = get("agent");
            doc.with_proxy = get("with-proxy") == "true";
            doc.self_proxy = get("self-proxy") == "true";
            doc.accept_invalid_certs = get("accept-invalid-certs") == "true";
            doc.updated_at = get("updated").parse().ok();
            doc.home = get("home");
            doc.usage = if get("usage-source") == "subscription-header" {
                get("usage-total")
                    .parse::<u64>()
                    .ok()
                    .filter(|total| *total > 0)
                    .map(|total_bytes| TrafficUsage {
                        used_bytes: get("usage-used").parse().unwrap_or(0),
                        total_bytes,
                        expires: Some(get("usage-expires")).filter(|value| !value.is_empty()),
                    })
            } else {
                None
            };
            doc
        })
        .collect();
    Some((
        documents,
        values.get("active").and_then(|id| id.parse().ok()),
    ))
}
pub(crate) fn values(documents: &[Document], active: Option<u64>) -> BTreeMap<String, String> {
    let mut values = BTreeMap::from([
        ("format".into(), "1".into()),
        (
            "ids".into(),
            documents
                .iter()
                .map(|d| d.id.to_string())
                .collect::<Vec<_>>()
                .join(","),
        ),
    ]);
    if let Some(active) = active {
        values.insert("active".into(), active.to_string());
    }
    for doc in documents {
        if let Some(usage) = &doc.usage {
            values.insert(
                format!("{}/usage-source", doc.id),
                "subscription-header".into(),
            );
            values.insert(
                format!("{}/usage-used", doc.id),
                usage.used_bytes.to_string(),
            );
            values.insert(
                format!("{}/usage-total", doc.id),
                usage.total_bytes.to_string(),
            );
            values.insert(
                format!("{}/usage-expires", doc.id),
                usage.expires.clone().unwrap_or_default(),
            );
        }
        for (field, value) in [
            ("name", doc.name.clone()),
            ("kind", doc.kind.label().into()),
            ("url", doc.url.clone()),
            ("content", doc.content.clone()),
            ("enabled", doc.enabled.to_string()),
            (
                "scope-profile",
                doc.scope_profile
                    .map(|id| id.to_string())
                    .unwrap_or_default(),
            ),
            ("scope-name", doc.scope_name.clone()),
            ("interval", doc.interval.clone()),
            ("timeout", doc.timeout.clone()),
            ("agent", doc.user_agent.clone()),
            ("with-proxy", doc.with_proxy.to_string()),
            ("self-proxy", doc.self_proxy.to_string()),
            ("accept-invalid-certs", doc.accept_invalid_certs.to_string()),
            (
                "updated",
                doc.updated_at
                    .map(|value| value.to_string())
                    .unwrap_or_default(),
            ),
            ("home", doc.home.clone()),
        ] {
            values.insert(format!("{}/{field}", doc.id), value);
        }
    }
    values
}

/// All profile file locations share the same storage policy as preferences.
pub(super) fn document_path(
    overrides: bool,
    document: &Document,
    cx: &gpui_kit::App,
) -> Result<PathBuf, String> {
    storage::document_path(overrides, document.id, document.kind.extension(), cx)
}

pub(super) fn read_document(path: PathBuf) -> Result<(String, String, bool), String> {
    let metadata = fs::metadata(&path).map_err(|e| format!("无法读取文件：{e}"))?;
    if metadata.len() > 2 * 1024 * 1024 {
        return Err("文件超过 2 MB，请选择较小的配置文件".into());
    }
    let extension = path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_lowercase();
    if !["yaml", "yml", "js"].contains(&extension.as_str()) {
        return Err("仅支持 .yaml、.yml 或 .js 文件".into());
    }
    let content = fs::read_to_string(&path).map_err(|e| format!("无法读取 UTF-8 配置：{e}"))?;
    Ok((
        path.file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned(),
        content,
        extension == "js",
    ))
}
