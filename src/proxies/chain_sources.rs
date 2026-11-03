//! Provider definitions are resolved from current source/cache, never healthcheck summaries.
use serde::{Deserialize, Serialize};
use serde_yaml_ng::{Mapping, Value};
use sha2::{Digest as _, Sha256};
use std::{collections::BTreeMap, path::Path};

/// Rule-set providers routinely exceed the configuration input limit.
pub(crate) const PROVIDER_CACHE_LIMIT: u64 = 64 * 1024 * 1024;

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
struct ProviderNode {
    source_name: String,
    definition: Value,
}
fn provider_nodes(provider: &Value, directory: &Path) -> Result<Vec<ProviderNode>, String> {
    let nodes = raw_provider_nodes(provider, directory)?;
    let filters = provider
        .get("filter")
        .and_then(Value::as_str)
        .unwrap_or("")
        .split('`')
        .map(provider_regex)
        .collect::<Result<Vec<_>, _>>()?;
    let exclusions = provider
        .get("exclude-filter")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(|value| {
            value
                .split('`')
                .map(provider_regex)
                .collect::<Result<Vec<_>, _>>()
        })
        .transpose()?
        .unwrap_or_default();
    let excluded_types = provider
        .get("exclude-type")
        .and_then(Value::as_str)
        .unwrap_or("")
        .split('|')
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>();
    let override_map = provider
        .get("override")
        .map(|value| value.as_mapping().ok_or("Provider override需为对象"))
        .transpose()?;
    let mut replacements = Vec::new();
    if let Some(overrides) = override_map {
        for (key, value) in overrides {
            let key = key.as_str().ok_or("Provider override字段名无效")?;
            let valid = match key {
                "tfo" | "mptcp" | "udp" | "udp-over-tcp" | "skip-cert-verify" => value.is_bool(),
                "up" | "down" | "dialer-proxy" | "name-cert-verify" | "interface-name"
                | "ip-version" | "additional-prefix" | "additional-suffix" => value.is_string(),
                "routing-mark" => value.as_i64().is_some(),
                "proxy-name" => value.is_sequence(),
                _ => return Err(format!("代理链暂不支持物化Provider override字段：{key}")),
            };
            if !valid {
                return Err(format!("Provider override.{key}类型无效"));
            }
        }
        if let Some(rules) = overrides.get("proxy-name").and_then(Value::as_sequence) {
            for rule in rules {
                let pattern = rule
                    .get("pattern")
                    .and_then(Value::as_str)
                    .ok_or("Provider重命名规则缺少pattern")?;
                let target = rule
                    .get("target")
                    .and_then(Value::as_str)
                    .ok_or("Provider重命名规则缺少target")?;
                let regex = provider_regex(pattern)?;
                let replacement = provider_replacement(&regex, target)?;
                replacements.push((regex, replacement));
            }
        }
    }
    let mut seen = std::collections::BTreeSet::new();
    let mut result = Vec::new();
    // Mihomo applies ordered filter alternatives to raw names, then overrides and renaming.
    for filter in filters {
        for node in &nodes {
            let source_name = node
                .get("name")
                .and_then(Value::as_str)
                .ok_or("Provider节点缺少名称")?;
            let kind = node
                .get("type")
                .and_then(Value::as_str)
                .ok_or("Provider节点缺少类型")?;
            if seen.contains(source_name)
                || !filter.is_match(source_name)
                || exclusions.iter().any(|regex| regex.is_match(source_name))
                || excluded_types
                    .iter()
                    .any(|excluded| excluded.eq_ignore_ascii_case(kind))
            {
                continue;
            }
            let mut definition = node.as_mapping().cloned().ok_or("Provider节点需为对象")?;
            if let Some(dialer) = provider
                .get("dialer-proxy")
                .and_then(Value::as_str)
                .filter(|value| !value.is_empty())
            {
                definition.insert("dialer-proxy".into(), dialer.into());
            }
            let mut name = source_name.to_owned();
            if let Some(overrides) = override_map {
                for (key, value) in overrides {
                    if !matches!(
                        key.as_str(),
                        Some("proxy-name" | "additional-prefix" | "additional-suffix")
                    ) {
                        definition.insert(key.clone(), value.clone());
                    }
                }
                for (regex, replacement) in &replacements {
                    name = regex.replace_all(&name, replacement.as_str()).into_owned();
                }
                if let Some(prefix) = overrides.get("additional-prefix").and_then(Value::as_str) {
                    name.insert_str(0, prefix);
                }
                if let Some(suffix) = overrides.get("additional-suffix").and_then(Value::as_str) {
                    name.push_str(suffix);
                }
            }
            definition.insert("name".into(), name.into());
            seen.insert(source_name.to_owned());
            result.push(ProviderNode {
                source_name: source_name.into(),
                definition: definition.into(),
            });
        }
    }
    Ok(result)
}
fn provider_regex(pattern: &str) -> Result<regex::Regex, String> {
    regex::Regex::new(pattern)
        .map_err(|_| "Provider正则无法用于链物化；不支持的高级表达式需改为普通节点定义".into())
}
// Normalize the supported .NET replacement tokens instead of guessing Rust's $name grammar.
fn provider_replacement(regex: &regex::Regex, target: &str) -> Result<String, String> {
    let mut result = String::new();
    let mut chars = target.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch != '$' {
            result.push(ch);
            continue;
        }
        match chars.peek().copied() {
            Some('$') => {
                chars.next();
                result.push_str("$$");
            }
            Some('{') => {
                chars.next();
                let mut name = String::new();
                loop {
                    match chars.next() {
                        Some('}') => break,
                        Some(ch) => name.push(ch),
                        None => return Err("Provider重命名替换表达式无效".into()),
                    }
                }
                if !regex
                    .capture_names()
                    .flatten()
                    .any(|capture| capture == name)
                    && !name
                        .parse::<usize>()
                        .is_ok_and(|index| index < regex.captures_len())
                {
                    return Err("Provider重命名引用的捕获组不存在".into());
                }
                result.push_str(&format!("${{{name}}}"));
            }
            Some(ch) if ch.is_ascii_digit() => {
                let mut digits = String::new();
                while chars.peek().is_some_and(char::is_ascii_digit) {
                    if let Some(digit) = chars.next() {
                        digits.push(digit);
                    }
                }
                if !digits
                    .parse::<usize>()
                    .is_ok_and(|index| index < regex.captures_len())
                {
                    return Err("Provider重命名引用的捕获组不存在".into());
                }
                result.push_str(&format!("${{{digits}}}"));
            }
            Some('&' | '`' | '\'' | '+' | '_') => {
                return Err("代理链暂不支持此Provider重命名替换表达式".into());
            }
            _ => result.push_str("$$"),
        }
    }
    Ok(result)
}
fn raw_provider_nodes(provider: &Value, directory: &Path) -> Result<Vec<Value>, String> {
    if provider.get("type").and_then(Value::as_str) == Some("inline") {
        return provider
            .get("payload")
            .and_then(Value::as_sequence)
            .cloned()
            .ok_or_else(|| "内联Provider缺少payload列表".into());
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
        .take(PROVIDER_CACHE_LIMIT + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "无法读取 Provider 缓存")?;
    if bytes.len() as u64 > PROVIDER_CACHE_LIMIT {
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
                for resolved in nodes {
                    let source_name = resolved.source_name;
                    let mut node = resolved.definition;
                    if let Some(node_name) =
                        node.get("name").and_then(Value::as_str).map(str::to_owned)
                    {
                        let occurrence = occurrences.entry(node_name.clone()).or_default();
                        let alias = if *occurrence == 0 {
                            format!("{name} · {node_name}")
                        } else {
                            format!("{name} · {node_name} · {}", *occurrence + 1)
                        };
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
                                node: source_name,
                                occurrence: 0,
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
            .filter(|node| node.source_name == source.node)
            .nth(source.occurrence)
            .ok_or_else(|| {
                format!(
                    "链式 Provider {} 的节点已不存在：{}（{}）",
                    source.provider,
                    source.node,
                    source.occurrence + 1
                )
            })?
            .definition
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
    fn repeated_raw_names_match_the_cores_first_definition_and_removed_members_fail() {
        let root:Value = serde_yaml_ng::from_str("proxy-providers: {p: {type: inline, payload: [{name: n, type: socks5, server: first}, {name: n, type: socks5, server: second}]}}").unwrap();
        let (definitions, sources, errors) = candidates(&root, Path::new("."));
        assert!(errors.is_empty());
        assert_eq!(definitions.len(), 1);
        let binding = BTreeMap::from([("p · n".into(), sources["p · n"].clone())]);
        let resolved = resolve(root.as_mapping().unwrap(), Path::new("."), &binding).unwrap();
        assert_eq!(resolved[0]["server"].as_str(), Some("first"));
        let mut missing = root;
        missing["proxy-providers"]["p"]["payload"] = Value::Sequence(vec![]);
        assert!(resolve(missing.as_mapping().unwrap(), Path::new("."), &binding).is_err());
    }
    #[test]
    fn provider_rebinding_reads_current_definition_and_keeps_same_name_sources_separate() {
        let directory = tempfile::tempdir().unwrap();
        let root:Value=serde_yaml_ng::from_str("proxy-providers: {a: {type: file, path: a.yaml}, b: {type: inline, payload: [{name: same, type: socks5, server: b}]}}").unwrap();
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
    #[test]
    fn provider_overrides_keep_raw_source_identity_and_chain_dialer_wins() {
        let mut root: Value = serde_yaml_ng::from_str(
            r#"
proxies: [{name: entry, type: socks5, server: entry}]
proxy-groups: [{name: target, type: select, use: [p]}]
proxy-providers:
  p:
    type: inline
    payload: [{name: HK-1, type: socks5, server: host, password: '  secret  ', udp: false}]
    dialer-proxy: original
    override:
      udp: true
      skip-cert-verify: true
      interface-name: en0
      routing-mark: 42
      proxy-name: [{pattern: '^HK-(.*)$', target: 'Hong Kong $1_node'}]
      additional-prefix: 'A '
      additional-suffix: ' Z'
"#,
        )
        .unwrap();
        let (definitions, sources, errors) = candidates(&root, Path::new("."));
        assert!(errors.is_empty(), "{errors:?}");
        let alias = "p · A Hong Kong 1_node Z";
        assert_eq!(sources[alias].node, "HK-1");
        assert_eq!(definitions[alias]["udp"], true);
        assert_eq!(definitions[alias]["password"], "  secret  ");
        assert_eq!(definitions[alias]["interface-name"], "en0");
        assert_eq!(definitions[alias]["routing-mark"], 42);
        let config = super::super::chain_config::Config {
            enabled: true,
            group: "target".into(),
            nodes: vec!["entry".into(), alias.into()],
            custom: String::new(),
            sources: BTreeMap::from([(alias.into(), sources[alias].clone())]),
        };
        root["proxy-providers"]["p"]["override"]["additional-prefix"] = "Changed ".into();
        config
            .apply_from(root.as_mapping_mut().unwrap(), Path::new("."))
            .unwrap();
        let materialized = root["proxies"]
            .as_sequence()
            .unwrap()
            .iter()
            .find(|node| node["name"] == alias)
            .unwrap();
        assert_eq!(materialized["dialer-proxy"], "entry");
        assert_eq!(materialized["skip-cert-verify"], true);
        assert_eq!(materialized["udp"], true);
        assert_eq!(root["proxy-providers"]["p"]["payload"][0]["name"], "HK-1");
        assert_eq!(root["proxy-providers"]["p"]["payload"][0]["udp"], false);
    }
    #[test]
    fn filters_apply_before_renaming_and_use_ordered_alternatives() {
        let root:Value=serde_yaml_ng::from_str(r#"proxy-providers:
  p:
    type: inline
    payload: [{name: HK-1, type: socks5}, {name: HK-no, type: socks5}, {name: US-1, type: ss}, {name: HK-2, type: http}]
    filter: 'US`HK'
    exclude-filter: 'no'
    exclude-type: 'HTTP'
    override: {additional-prefix: XX}
"#).unwrap();
        let nodes = provider_nodes(&root["proxy-providers"]["p"], Path::new(".")).unwrap();
        assert_eq!(
            nodes
                .iter()
                .map(|node| node.source_name.as_str())
                .collect::<Vec<_>>(),
            ["US-1", "HK-1"]
        );
        assert_eq!(nodes[0].definition["name"], "XXUS-1");
    }
    #[test]
    fn confirmed_remote_cache_takes_precedence_over_bootstrap_payload() {
        let directory = tempfile::tempdir().unwrap();
        let root:Value=serde_yaml_ng::from_str("proxy-providers: {p: {type: http, url: 'https://example.test/nodes', path: p.yaml, payload: [{name: old, type: socks5}], override: {udp: true}}}").unwrap();
        let bytes = b"proxies: [{name: current, type: socks5, server: active}]";
        publish_cache(
            &directory.path().join("p.yaml"),
            "https://example.test/nodes",
            bytes,
        )
        .unwrap();
        let (definitions, sources, errors) = candidates(&root, directory.path());
        assert!(errors.is_empty());
        assert!(!definitions.contains_key("p · old"));
        assert_eq!(definitions["p · current"]["udp"], true);
        assert_eq!(sources["p · current"].node, "current");
    }
    #[test]
    fn unsupported_provider_expressions_fail_before_mutating_chain_config() {
        for overrides in [
            "{proxy-name: [{pattern: '(?=HK)HK', target: X}]}",
            "{proxy-name: [{pattern: '(HK)', target: '$2'}]}",
            "{override-expr: ['true']}",
        ] {
            let mut root:Value=serde_yaml_ng::from_str(&format!("proxies: [{{name: entry,type: socks5}}]\nproxy-groups: [{{name: target,type: select,use: [p]}}]\nproxy-providers: {{p: {{type: inline,payload: [{{name: HK,type: socks5}}],override: {overrides}}}}}")).unwrap();
            let before = root.clone();
            let config = super::super::chain_config::Config {
                enabled: true,
                group: "target".into(),
                nodes: vec!["entry".into(), "bound".into()],
                custom: String::new(),
                sources: BTreeMap::from([(
                    "bound".into(),
                    Source {
                        provider: "p".into(),
                        node: "HK".into(),
                        occurrence: 0,
                    },
                )]),
            };
            assert!(
                config
                    .apply_from(root.as_mapping_mut().unwrap(), Path::new("."))
                    .is_err()
            );
            assert_eq!(root, before);
        }
    }
    #[test]
    fn renamed_display_collisions_keep_distinct_raw_source_bindings() {
        let root: Value = serde_yaml_ng::from_str(
            r#"proxy-providers:
  p:
    type: inline
    payload: [{name: a, type: socks5, server: first}, {name: b, type: socks5, server: second}]
    override: {proxy-name: [{pattern: '^[ab]$', target: x}]}
"#,
        )
        .unwrap();
        let (_, sources, errors) = candidates(&root, Path::new("."));
        assert!(errors.is_empty(), "{errors:?}");
        assert_eq!(sources["p · x"].node, "a");
        assert_eq!(sources["p · x · 2"].node, "b");
        assert_eq!(sources["p · x · 2"].occurrence, 0);
        let resolved = resolve(root.as_mapping().unwrap(), Path::new("."), &sources).unwrap();
        assert_eq!(
            resolved
                .iter()
                .find(|node| node["name"] == "p · x")
                .unwrap()["server"],
            "first"
        );
        assert_eq!(
            resolved
                .iter()
                .find(|node| node["name"] == "p · x · 2")
                .unwrap()["server"],
            "second"
        );
    }
}
