//! Ordered dialer links. Saved preferences and generated runtime remain separate.
use crate::storage::AppPaths;
use serde::{Deserialize, Serialize};
use serde_yaml_ng::{Mapping, Value};
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct Config {
    pub(crate) enabled: bool,
    pub(crate) group: String,
    pub(crate) nodes: Vec<String>,
    #[serde(
        serialize_with = "serialize_custom",
        deserialize_with = "deserialize_custom"
    )]
    pub(crate) custom: String,
    pub(crate) sources: std::collections::BTreeMap<String, super::chain_sources::Source>,
}
#[derive(Clone, Debug)]
pub(crate) struct Change {
    pub(crate) expected: Config,
    pub(crate) desired: Config,
    pub(crate) generation: u64,
    pub(crate) epoch: u64,
}
fn serialize_custom<S: serde::Serializer>(text: &str, serializer: S) -> Result<S::Ok, S::Error> {
    let value = if text.trim().is_empty() {
        serde_yaml_ng::from_str::<Value>("proxies: []").map_err(serde::ser::Error::custom)?
    } else {
        serde_yaml_ng::from_str(text).map_err(serde::ser::Error::custom)?
    };
    value.serialize(serializer)
}
fn deserialize_custom<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<String, D::Error> {
    let value = Value::deserialize(deserializer)?;
    let map = value
        .as_mapping()
        .ok_or_else(|| serde::de::Error::custom("custom 需为对象"))?;
    if map.len() == 1
        && map
            .get("proxies")
            .and_then(Value::as_sequence)
            .is_some_and(Vec::is_empty)
    {
        return Ok(String::new());
    }
    serde_yaml_ng::to_string(&value).map_err(serde::de::Error::custom)
}
impl Config {
    pub(crate) fn existing(root: &Value, exit: &str) -> Result<Vec<String>, String> {
        let Some(definitions) = root.get("proxies").and_then(Value::as_sequence) else {
            return Ok(Vec::new());
        };
        let mut nodes = Vec::new();
        let mut current = exit;
        loop {
            if nodes.iter().any(|name| name == current) || nodes.len() >= 32 {
                return Err("来源代理链存在循环或超过节点上限".into());
            }
            let node = definitions
                .iter()
                .find(|node| node.get("name").and_then(Value::as_str) == Some(current));
            let Some(node) = node else {
                return if nodes.is_empty() {
                    Ok(Vec::new())
                } else {
                    Err("来源代理链含未解析节点".into())
                };
            };
            nodes.push(current.to_owned());
            match node.get("dialer-proxy").and_then(Value::as_str) {
                Some(next) => current = next,
                None => break,
            }
        }
        nodes.reverse();
        Ok(if nodes.len() > 1 { nodes } else { Vec::new() })
    }

    #[cfg(test)]
    pub(crate) fn path(paths: &AppPaths) -> std::path::PathBuf {
        crate::preferences::app::path(paths)
    }
    pub(crate) fn load(paths: &AppPaths) -> Result<Self, String> {
        let value: Self = crate::preferences::app::read(paths, "proxy_chain")?;
        value.validate()?;
        Ok(value)
    }
    pub(crate) fn save(&self, paths: &AppPaths) -> Result<(), String> {
        self.validate()?;
        crate::preferences::app::write(paths, "proxy_chain", self)
    }
    pub(crate) fn custom_nodes(&self) -> Result<Vec<Value>, String> {
        if self.custom.trim().is_empty() {
            return Ok(vec![]);
        }
        let root: Value = serde_yaml_ng::from_str(&self.custom)
            .map_err(|error| crate::logging::diagnostic_error("自定义链式 YAML 无效", error))?;
        let map = root.as_mapping().ok_or("自定义链式配置需为 proxies 对象")?;
        if map.len() != 1 || !map.contains_key("proxies") {
            return Err("自定义链式配置仅支持 proxies".into());
        }
        let proxies = map["proxies"]
            .as_sequence()
            .ok_or("自定义 proxies 需为列表")?;
        for proxy in proxies {
            if proxy.get("name").and_then(Value::as_str).is_none()
                || proxy.get("type").and_then(Value::as_str).is_none()
            {
                return Err("自定义节点缺少名称或类型".into());
            }
        }
        Ok(proxies.clone())
    }
    pub(crate) fn effective_nodes(&self) -> Result<Vec<String>, String> {
        self.custom_nodes()?;
        Ok(self.nodes.clone())
    }
    pub(crate) fn validate(&self) -> Result<(), String> {
        if self.group.len() > 1024 {
            return Err("链式目标组名称超过上限".into());
        }
        if self.custom.len() > 512 * 1024
            || self.nodes.len() > 32
            || self
                .nodes
                .iter()
                .any(|name| name.is_empty() || name.len() > 1024)
        {
            return Err("链式代理设置超过上限".into());
        }
        let nodes = self.effective_nodes()?;
        let unique = nodes.iter().collect::<std::collections::BTreeSet<_>>();
        if unique.len() != nodes.len() || nodes.len() > 32 {
            return Err("链式代理节点重复或超过上限".into());
        }
        if self.enabled && (nodes.len() < 2 || self.group.is_empty()) {
            return Err("选择目标代理组及至少两个不同节点".into());
        }
        Ok(())
    }
    #[cfg(test)]
    pub(crate) fn apply(&self, root: &mut Mapping) -> Result<(), String> {
        self.apply_from(root, std::path::Path::new("."))
    }
    pub(crate) fn apply_from(
        &self,
        root: &mut Mapping,
        directory: &std::path::Path,
    ) -> Result<(), String> {
        self.validate()?;
        if !self.enabled {
            return Ok(());
        }
        let mut custom = self.custom_nodes()?;
        let bindings = self
            .sources
            .iter()
            .filter(|(name, _)| self.nodes.contains(name))
            .map(|(name, source)| (name.clone(), source.clone()))
            .collect();
        let resolved = super::chain_sources::resolve(root, directory, &bindings)?;
        custom.retain(|proxy| {
            !proxy
                .get("name")
                .and_then(Value::as_str)
                .is_some_and(|name| self.sources.contains_key(name))
        });
        custom.extend(resolved);
        if !root.contains_key("proxies") {
            root.insert("proxies".into(), Value::Sequence(vec![]));
        }
        let proxies = root
            .get_mut("proxies")
            .and_then(Value::as_sequence_mut)
            .ok_or("运行 proxies 不是列表")?;
        for proxy in custom {
            let name = proxy
                .get("name")
                .and_then(Value::as_str)
                .ok_or("链式节点名称无效")?;
            if let Some(index) = proxies
                .iter()
                .position(|value| value.get("name").and_then(Value::as_str) == Some(name))
            {
                proxies[index] = proxy;
            } else {
                proxies.push(proxy);
            }
        }
        let nodes = self.effective_nodes()?;
        for (index, name) in nodes.iter().enumerate() {
            let proxy = proxies
                .iter_mut()
                .find(|proxy| proxy.get("name").and_then(Value::as_str) == Some(name))
                .ok_or_else(|| format!("链式节点不存在：{name}"))?;
            let map = proxy.as_mapping_mut().ok_or("链式节点不是对象")?;
            if index == 0 {
                map.remove("dialer-proxy");
            } else {
                map.insert("dialer-proxy".into(), nodes[index - 1].clone().into());
            }
        }
        let exit = nodes.last().ok_or("链式节点为空")?;
        if self.group != "GLOBAL" {
            let group = root
                .get_mut("proxy-groups")
                .and_then(Value::as_sequence_mut)
                .and_then(|groups| {
                    groups.iter_mut().find(|group| {
                        group.get("name").and_then(Value::as_str) == Some(&self.group)
                    })
                })
                .ok_or("链式目标代理组不存在")?;
            if !matches!(
                group.get("type").and_then(Value::as_str),
                Some("select" | "url-test")
            ) {
                return Err("链式目标需为手动选择或延迟优选组".into());
            }
            if !group
                .as_mapping()
                .ok_or("目标代理组不是对象")?
                .contains_key("proxies")
            {
                let provider_members = group
                    .get("use")
                    .and_then(Value::as_sequence)
                    .is_some_and(|providers| !providers.is_empty())
                    || ["include-all", "include-all-providers"]
                        .iter()
                        .any(|key| group.get(*key).and_then(Value::as_bool) == Some(true));
                if !provider_members {
                    return Err("目标代理组没有成员来源".into());
                }
                group
                    .as_mapping_mut()
                    .ok_or("目标代理组不是对象")?
                    .insert("proxies".into(), Value::Sequence(Vec::new()));
            }
            let members = group
                .get_mut("proxies")
                .and_then(Value::as_sequence_mut)
                .ok_or("目标代理组没有成员列表")?;
            if !members.iter().any(|member| member.as_str() == Some(exit)) {
                members.push(exit.clone().into());
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn provider_only_target_keeps_its_sources_and_gains_the_materialized_exit() {
        use super::Config;
        use serde_yaml_ng::{Mapping, Value};
        for members in [
            "use: [remote]",
            "include-all-providers: true",
            "include-all: true",
        ] {
            let mut root: Mapping = serde_yaml_ng::from_str(&format!("proxies: [{{name: entry, type: socks5}}, {{name: exit, type: socks5}}]\nproxy-groups: [{{name: provider-group, type: select, {members}}}]\nproxy-providers: {{remote: {{type: http, url: 'https://example.test/nodes', path: remote.yaml}}}}\n")).unwrap();
            let before = root["proxy-groups"][0].clone();
            Config {
                enabled: true,
                group: "provider-group".into(),
                nodes: vec!["entry".into(), "exit".into()],
                ..Default::default()
            }
            .apply(&mut root)
            .unwrap();
            let group = &root["proxy-groups"][0];
            assert_eq!(group["proxies"], Value::Sequence(vec!["exit".into()]));
            for (key, value) in before.as_mapping().unwrap() {
                assert_eq!(&group[key], value);
            }
            assert_eq!(root["proxies"][1]["dialer-proxy"], "entry");
        }
    }

    use super::*;
    #[test]
    fn ordered_links_preserve_sources_and_custom_definitions_follow_visual_order() {
        let source:Value=serde_yaml_ng::from_str("proxies:\n- {name: a, type: socks5, server: localhost, port: 1080}\n- {name: b, type: socks5, server: localhost, port: 1081}\n- {name: other, type: socks5, dialer-proxy: existing}\nproxy-groups:\n- {name: select, type: select, proxies: [a]}\n").unwrap();
        let mut output = source.as_mapping().unwrap().clone();
        let chain = Config {
            enabled: true,
            group: "select".into(),
            nodes: vec!["a".into(), "b".into()],
            custom: String::new(),
            sources: Default::default(),
        };
        chain.apply(&mut output).unwrap();
        let proxies = output["proxies"].as_sequence().unwrap();
        assert_eq!(proxies[1]["dialer-proxy"].as_str(), Some("a"));
        assert!(proxies[0].get("dialer-proxy").is_none());
        assert_eq!(proxies[2]["dialer-proxy"].as_str(), Some("existing"));
        assert!(source["proxies"][1].get("dialer-proxy").is_none());
        let custom=Config{nodes:vec!["c".into(),"d".into(),"e".into()],custom:"proxies:\n- {name: c, type: socks5}\n- {name: d, type: socks5}\n- {name: e, type: socks5}".into(),..chain};
        assert_eq!(custom.effective_nodes().unwrap(), ["c", "d", "e"]);
        assert!(
            Config {
                custom: "rules: []".into(),
                ..custom
            }
            .validate()
            .is_err()
        );
    }
    #[test]
    fn custom_yaml_does_not_override_visual_reorder_or_removed_nodes() {
        let chain=Config { enabled:true, group:"g".into(), nodes:vec!["b".into(),"a".into()], custom:"proxies: [{name: a, type: socks5}, {name: b, type: socks5}, {name: c, type: socks5}]".into(),sources:Default::default() };
        let mut root = serde_yaml_ng::from_str::<Mapping>(
            "proxy-groups: [{name: g, type: url-test, proxies: [a,b,c]}]",
        )
        .unwrap();
        chain.apply(&mut root).unwrap();
        assert_eq!(chain.effective_nodes().unwrap(), ["b", "a"]);
        let proxies = root["proxies"].as_sequence().unwrap();
        assert_eq!(proxies[0]["dialer-proxy"].as_str(), Some("b"));
        assert!(proxies[1].get("dialer-proxy").is_none());
        assert!(proxies[2].get("dialer-proxy").is_none());
    }
    #[test]
    fn existing_chain_reverses_links_and_rejects_cycles_or_missing_relay() {
        let root: Value = serde_yaml_ng::from_str(
            "proxies: [{name: a}, {name: b, dialer-proxy: a}, {name: c, dialer-proxy: b}]",
        )
        .unwrap();
        assert_eq!(Config::existing(&root, "c").unwrap(), ["a", "b", "c"]);
        assert!(Config::existing(&root, "DIRECT").unwrap().is_empty());
        let mut invalid = root.clone();
        invalid["proxies"][0]["dialer-proxy"] = "c".into();
        assert!(Config::existing(&invalid, "c").is_err());
        invalid["proxies"][0]["dialer-proxy"] = "missing".into();
        assert!(Config::existing(&invalid, "c").is_err());
    }
    #[test]
    fn missing_defaults_and_invalid_json_do_not_overwrite_user_file() {
        let root = tempfile::tempdir().unwrap();
        let paths = AppPaths::isolated(root.path());
        paths.create().unwrap();
        assert_eq!(Config::load(&paths).unwrap(), Config::default());
        Config::default().save(&paths).unwrap();
        let root: serde_json::Value =
            serde_json::from_slice(&std::fs::read(Config::path(&paths)).unwrap()).unwrap();
        let json = &root["proxy_chain"];
        assert!(json["enabled"].is_boolean());
        assert!(json["nodes"].is_array());
        assert!(json["custom"].is_object());
        assert!(json["custom"]["proxies"].is_array());
        std::fs::write(Config::path(&paths), b"invalid").unwrap();
        assert!(Config::load(&paths).is_err());
        assert_eq!(std::fs::read(Config::path(&paths)).unwrap(), b"invalid");
    }
}
