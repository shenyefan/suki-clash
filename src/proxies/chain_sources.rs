//! Provider definitions are resolved from current source/cache, never healthcheck summaries.
use serde::{Deserialize, Serialize};
use serde_yaml_ng::{Mapping, Value};
use sha2::{Digest as _, Sha256};
use std::{collections::BTreeMap, path::Path};

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct CacheSource {
    url_sha256: String,
    content_sha256: String,
}
fn digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}
fn proof_path(path: &Path) -> std::path::PathBuf {
    path.with_file_name(format!(
        ".{}.suki-source.json",
        path.file_name().unwrap_or_default().to_string_lossy()
    ))
}
/// A local core owns its cache bytes; confirming provenance must never replace them.
pub(crate) fn confirm_local_cache(path: &Path, url: &str, bytes: &[u8]) -> Result<bool, String> {
    let current =
        std::fs::read(path).map_err(|error| format!("无法确认 Provider 缓存：{error}"))?;
    if current != bytes {
        return Ok(false);
    }
    let encoded = serde_json::to_vec(&CacheSource {
        url_sha256: digest(url.as_bytes()),
        content_sha256: digest(bytes),
    })
    .map_err(|_| "无法编码 Provider 来源证明")?;
    crate::storage::atomic_write(&proof_path(path), &encoded).map_err(|error| error.to_string())?;
    Ok(true)
}
/// Publish a verified cache and its source proof without storing a secret-bearing URL.
pub(crate) fn publish_cache(path: &Path, url: &str, bytes: &[u8]) -> Result<(), String> {
    let proof = proof_path(path);
    let previous = match std::fs::read(path) {
        Ok(bytes) => Some(bytes),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(format!("无法保留 Provider 缓存：{error}")),
    };
    let encoded = serde_json::to_vec(&CacheSource {
        url_sha256: digest(url.as_bytes()),
        content_sha256: digest(bytes),
    })
    .map_err(|_| "无法编码 Provider 来源证明")?;
    let changed = previous.as_deref() != Some(bytes);
    if changed {
        crate::storage::atomic_write(path, bytes).map_err(|error| error.to_string())?;
    }
    if let Err(error) = crate::storage::atomic_write(&proof, &encoded) {
        let recovery = if !changed {
            Ok(())
        } else {
            match previous {
                Some(previous) => {
                    crate::storage::atomic_write(path, &previous).map_err(|error| error.to_string())
                }
                None => std::fs::remove_file(path).map_err(|error| error.to_string()),
            }
        };
        return Err(match recovery {
            Ok(()) => format!("Provider 来源证明未提交：{error}"),
            Err(recovery) => {
                format!("Provider 来源证明未提交：{error}；原缓存恢复失败：{recovery}")
            }
        });
    }
    Ok(())
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Source {
    pub(crate) provider: String,
    pub(crate) node: String,
    pub(crate) occurrence: usize,
}
fn provider_nodes(provider: &Value, directory: &Path) -> Result<Vec<Value>, String> {
    if let Some(payload) = provider.get("payload").and_then(Value::as_sequence) {
        return Ok(payload.clone());
    }
    let path = provider
        .get("path")
        .and_then(Value::as_str)
        .ok_or("Provider 没有本地节点定义")?;
    let path = directory.join(path);
    use std::io::Read as _;
    let mut bytes = Vec::new();
    std::fs::File::open(&path)
        .map_err(|_| "Provider 缓存尚不可用")?
        .take(crate::configuration::INPUT_LIMIT + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "无法读取 Provider 缓存")?;
    if bytes.len() as u64 > crate::configuration::INPUT_LIMIT {
        return Err("Provider 缓存超过读取上限".into());
    }
    if provider.get("type").and_then(Value::as_str) == Some("http") {
        let url = provider
            .get("url")
            .and_then(Value::as_str)
            .ok_or("Provider URL 缺失")?;
        let proof: CacheSource = serde_json::from_slice(
            &std::fs::read(proof_path(&path))
                .map_err(|_| "Provider 缓存来源尚未确认，请先同步当前来源后重试代理链")?,
        )
        .map_err(|_| "Provider 缓存来源证明无效")?;
        if proof.url_sha256 != digest(url.as_bytes()) || proof.content_sha256 != digest(&bytes) {
            return Err("Provider 缓存与当前来源不一致，请同步后重试代理链".into());
        }
    }
    let value: Value = serde_yaml_ng::from_slice(&bytes).map_err(|_| "Provider 缓存 YAML 无效")?;
    value
        .get("proxies")
        .and_then(Value::as_sequence)
        .cloned()
        .ok_or_else(|| "Provider 缓存缺少 proxies 列表".into())
}
pub(crate) fn candidates(
    root: &Value,
    directory: &Path,
) -> (
    BTreeMap<String, Value>,
    BTreeMap<String, Source>,
    Vec<String>,
) {
    let mut definitions = BTreeMap::new();
    let mut sources = BTreeMap::new();
    let mut errors = Vec::new();
    for proxy in root
        .get("proxies")
        .and_then(Value::as_sequence)
        .into_iter()
        .flatten()
    {
        if let Some(name) = proxy.get("name").and_then(Value::as_str) {
            definitions.insert(name.into(), proxy.clone());
        }
    }
    for (name, provider) in root
        .get("proxy-providers")
        .and_then(Value::as_mapping)
        .into_iter()
        .flatten()
    {
        let Some(name) = name.as_str() else {
            continue;
        };
        match provider_nodes(provider, directory) {
            Ok(nodes) => {
                let mut occurrences = BTreeMap::<String, usize>::new();
                for mut node in nodes {
                    if let Some(node_name) =
                        node.get("name").and_then(Value::as_str).map(str::to_owned)
                    {
                        let occurrence = occurrences.entry(node_name.clone()).or_default();
                        let alias = if *occurrence == 0 {
                            format!("{name} · {node_name}")
                        } else {
                            format!("{name} · {node_name} · {}", *occurrence + 1)
                        };
                        let index = *occurrence;
                        *occurrence += 1;
                        if definitions.contains_key(&alias) {
                            errors.push(format!("Provider {name} 的节点身份重复：{node_name}"));
                            continue;
                        }
                        node["name"] = alias.clone().into();
                        sources.insert(
                            alias.clone(),
                            Source {
                                provider: name.into(),
                                node: node_name,
                                occurrence: index,
                            },
                        );
                        definitions.insert(alias, node);
                    }
                }
            }
            Err(error) => errors.push(format!("Provider {name}：{error}")),
        }
    }
    (definitions, sources, errors)
}
pub(crate) fn resolve(
    root: &Mapping,
    directory: &Path,
    bindings: &BTreeMap<String, Source>,
) -> Result<Vec<Value>, String> {
    if bindings.is_empty() {
        return Ok(Vec::new());
    }
    let mut result = Vec::new();
    let mut cache = BTreeMap::new();
    for (alias, source) in bindings {
        if !cache.contains_key(&source.provider) {
            let provider = root
                .get("proxy-providers")
                .and_then(Value::as_mapping)
                .and_then(|providers| providers.get(Value::String(source.provider.clone())))
                .ok_or_else(|| format!("链式 Provider 已不存在：{}", source.provider))?;
            cache.insert(
                source.provider.clone(),
                provider_nodes(provider, directory)?,
            );
        }
        let nodes = &cache[&source.provider];
        let node = nodes
            .iter()
            .filter(|node| node.get("name").and_then(Value::as_str) == Some(&source.node))
            .nth(source.occurrence)
            .ok_or_else(|| {
                format!(
                    "链式 Provider {} 的节点已不存在：{}（{}）",
                    source.provider,
                    source.node,
                    source.occurrence + 1
                )
            })?
            .clone();
        let mut node = node;
        node["name"] = alias.clone().into();
        result.push(node);
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn remote_cache_requires_matching_source_and_local_confirmation_keeps_the_file() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("cache.yaml");
        let bytes = b"proxies: [{name: n, type: socks5, server: old}]";
        std::fs::write(&path, bytes).unwrap();
        let modified = std::fs::metadata(&path).unwrap().modified().unwrap();
        let mut provider: Value = serde_yaml_ng::from_str(
            "type: http\nurl: https://example.test/sub?token=private\npath: cache.yaml",
        )
        .unwrap();
        assert!(provider_nodes(&provider, directory.path()).is_err());
        assert!(confirm_local_cache(&path, provider["url"].as_str().unwrap(), bytes).unwrap());
        assert_eq!(
            std::fs::metadata(&path).unwrap().modified().unwrap(),
            modified
        );
        assert!(provider_nodes(&provider, directory.path()).is_ok());
        assert!(
            !std::fs::read_to_string(proof_path(&path))
                .unwrap()
                .contains("private")
        );
        provider["url"] = "https://example.test/different".into();
        assert!(provider_nodes(&provider, directory.path()).is_err());
        std::fs::write(&path, b"proxies: []").unwrap();
        assert!(!confirm_local_cache(&path, "https://example.test/different", bytes).unwrap());
        assert_eq!(std::fs::read(&path).unwrap(), b"proxies: []");
    }
    #[test]
    fn failed_source_proof_publication_restores_the_previous_cache() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("cache.yaml");
        std::fs::write(&path, b"previous").unwrap();
        std::fs::create_dir(proof_path(&path)).unwrap();
        assert!(publish_cache(&path, "https://example.test/current", b"new").is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"previous");
    }
    #[test]
    fn duplicate_provider_names_bind_their_own_occurrence_and_missing_members_fail() {
        let root:Value = serde_yaml_ng::from_str("proxy-providers: {p: {payload: [{name: n, type: socks5, server: first}, {name: n, type: socks5, server: second}]}}").unwrap();
        let (definitions, sources, errors) = candidates(&root, Path::new("."));
        assert!(errors.is_empty());
        assert_eq!(definitions.len(), 2);
        let binding = BTreeMap::from([("p · n · 2".into(), sources["p · n · 2"].clone())]);
        let resolved = resolve(root.as_mapping().unwrap(), Path::new("."), &binding).unwrap();
        assert_eq!(resolved[0]["server"].as_str(), Some("second"));
        let mut missing = root;
        missing["proxy-providers"]["p"]["payload"]
            .as_sequence_mut()
            .unwrap()
            .pop();
        assert!(resolve(missing.as_mapping().unwrap(), Path::new("."), &binding).is_err());
    }
    #[test]
    fn provider_rebinding_reads_current_definition_and_keeps_same_name_sources_separate() {
        let directory = tempfile::tempdir().unwrap();
        let root:Value=serde_yaml_ng::from_str("proxy-providers: {a: {path: a.yaml}, b: {payload: [{name: same, type: socks5, server: b}]}}").unwrap();
        std::fs::write(
            directory.path().join("a.yaml"),
            "proxies: [{name: same, type: socks5, server: old}]",
        )
        .unwrap();
        let (definitions, sources, errors) = candidates(&root, directory.path());
        assert!(errors.is_empty());
        assert_eq!(definitions.len(), 2);
        assert_ne!(sources["a · same"], sources["b · same"]);
        std::fs::write(
            directory.path().join("a.yaml"),
            "proxies: [{name: same, type: socks5, server: new}]",
        )
        .unwrap();
        let nodes = resolve(
            root.as_mapping().unwrap(),
            directory.path(),
            &BTreeMap::from([("a · same".into(), sources["a · same"].clone())]),
        )
        .unwrap();
        assert_eq!(nodes[0]["server"].as_str(), Some("new"));
        std::fs::write(directory.path().join("a.yaml"), "proxies: []").unwrap();
        assert!(resolve(root.as_mapping().unwrap(), directory.path(), &sources).is_err());
    }
}
