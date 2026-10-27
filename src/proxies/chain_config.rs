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
        let root: Value =
            serde_yaml_ng::from_str(&self.custom).map_err(|_| "自定义链式 YAML 无效")?;
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
        let custom = self.custom_nodes()?;
        if custom.is_empty() {
            Ok(self.nodes.clone())
        } else {
            Ok(custom
                .iter()
                .filter_map(|proxy| proxy.get("name").and_then(Value::as_str))
                .map(str::to_owned)
                .collect())
        }
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
    pub(crate) fn apply(&self, root: &mut Mapping) -> Result<(), String> {
        self.validate()?;
        if !self.enabled {
            return Ok(());
        }
        let custom = self.custom_nodes()?;
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
            if group.get("type").and_then(Value::as_str) != Some("select") {
                return Err("链式目标需为手动选择组".into());
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
    use super::*;
    #[test]
    fn ordered_links_preserve_sources_and_custom_nodes_use_yaml_order() {
        let source:Value=serde_yaml_ng::from_str("proxies:\n- {name: a, type: socks5, server: localhost, port: 1080}\n- {name: b, type: socks5, server: localhost, port: 1081}\n- {name: other, type: socks5, dialer-proxy: existing}\nproxy-groups:\n- {name: select, type: select, proxies: [a]}\n").unwrap();
        let mut output = source.as_mapping().unwrap().clone();
        let chain = Config {
            enabled: true,
            group: "select".into(),
            nodes: vec!["a".into(), "b".into()],
            custom: String::new(),
        };
        chain.apply(&mut output).unwrap();
        let proxies = output["proxies"].as_sequence().unwrap();
        assert_eq!(proxies[1]["dialer-proxy"].as_str(), Some("a"));
        assert!(proxies[0].get("dialer-proxy").is_none());
        assert_eq!(proxies[2]["dialer-proxy"].as_str(), Some("existing"));
        assert!(source["proxies"][1].get("dialer-proxy").is_none());
        let custom=Config{custom:"proxies:\n- {name: c, type: socks5}\n- {name: d, type: socks5}\n- {name: e, type: socks5}".into(),..chain};
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
