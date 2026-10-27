//! Real Mihomo proxy identities and per-profile selections. No GPUI dependency.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum ProxyMode {
    Rule,
    Global,
    Direct,
}

impl ProxyMode {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Rule => "rule",
            Self::Global => "global",
            Self::Direct => "direct",
        }
    }
}
use crate::storage::{AppPaths, atomic_write};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
pub(crate) struct Proxy {
    pub(crate) name: String,
    #[serde(rename = "type")]
    pub(crate) kind: String,
    #[serde(default)]
    pub(crate) all: Option<Vec<String>>,
    #[serde(default)]
    pub(crate) now: Option<String>,
    #[serde(default)]
    pub(crate) history: Vec<Delay>,
    #[serde(default)]
    pub(crate) hidden: bool,
    #[serde(default)]
    pub(crate) fixed: Option<String>,
    #[serde(default, rename = "testUrl")]
    pub(crate) test_url: String,
    #[serde(default)]
    pub(crate) icon: String,
    #[serde(skip)]
    pub(crate) provider: Option<String>,
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
pub(crate) struct Delay {
    pub(crate) delay: u32,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Proxies {
    pub(crate) entries: BTreeMap<String, Proxy>,
    pub(crate) order: Vec<String>,
    pub(crate) measurements: BTreeMap<(String, String), Result<u32, String>>,
    pub(crate) testing: BTreeSet<(String, String)>,
}
impl Proxies {
    pub(crate) fn parse(bytes: &[u8], order: &[String]) -> Result<Self, String> {
        #[derive(Deserialize)]
        struct Response {
            proxies: BTreeMap<String, Proxy>,
        }
        let response: Response = serde_json::from_slice(bytes).map_err(|_| "内核代理响应无效")?;
        if response.proxies.len() > 20000 {
            return Err("内核代理列表超过上限".into());
        }
        let mut result = Self {
            entries: response.proxies,
            order: vec![],
            measurements: BTreeMap::new(),
            testing: Default::default(),
        };
        for (name, proxy) in &result.entries {
            if name != &proxy.name
                || name.len() > 1024
                || proxy.all.as_ref().is_some_and(|nodes| nodes.len() > 20000)
            {
                return Err("内核代理身份或成员无效".into());
            }
        }
        let groups = result
            .entries
            .iter()
            .filter(|(_, proxy)| proxy.all.is_some())
            .map(|(name, _)| name.clone())
            .collect::<BTreeSet<_>>();
        let mut added = BTreeSet::new();
        for name in order.iter().chain(groups.iter()) {
            if groups.contains(name) && added.insert(name.clone()) {
                result.order.push(name.clone());
            }
        }
        Ok(result)
    }
    pub(crate) fn include_providers(&mut self, bytes: &[u8]) -> Result<(), String> {
        #[derive(Deserialize)]
        struct Provider {
            #[serde(default)]
            proxies: Vec<Proxy>,
        }
        #[derive(Deserialize)]
        struct Response {
            providers: BTreeMap<String, Provider>,
        }
        let response: Response = serde_json::from_slice(bytes).map_err(|_| "代理集合响应无效")?;
        let mut additions = BTreeMap::<String, Proxy>::new();
        for (provider, data) in response.providers {
            for mut proxy in data.proxies {
                if proxy.name.is_empty() || proxy.name.len() > 1024 {
                    return Err("代理集合节点身份无效".into());
                }
                if self.entries.contains_key(&proxy.name) {
                    continue;
                }
                if additions.contains_key(&proxy.name) {
                    return Err("代理集合存在重名节点，无法确认来源".into());
                }
                proxy.provider = Some(provider.clone());
                additions.insert(proxy.name.clone(), proxy);
                if additions.len().saturating_add(self.entries.len()) > 20000 {
                    return Err("内核代理列表超过上限".into());
                }
            }
        }
        self.entries.extend(additions);
        Ok(())
    }
    pub(crate) fn testable(&self, group: &str, node: &str) -> bool {
        !matches!(
            node,
            "DIRECT" | "REJECT" | "REJECT-DROP" | "PASS" | "COMPATIBLE"
        ) && self.entries.contains_key(node)
            && self
                .entries
                .get(group)
                .and_then(|proxy| proxy.all.as_ref())
                .is_some_and(|members| members.iter().any(|member| member == node))
    }
    pub(crate) fn test_url<'a>(&'a self, group: &str, default: &'a str) -> &'a str {
        self.entries
            .get(group)
            .map(|proxy| proxy.test_url.trim())
            .filter(|url| !url.is_empty())
            .unwrap_or(default)
    }
    pub(crate) fn confirms_selection(&self, group: &str, node: &str) -> bool {
        self.entries
            .get(group)
            .is_some_and(|proxy| match proxy.kind.as_str() {
                "Selector" => proxy.now.as_deref() == Some(node),
                "URLTest" | "Fallback" => proxy.fixed.as_deref() == Some(node),
                _ => false,
            })
    }
    pub(crate) fn selected(&self, group: &str) -> Option<&str> {
        self.entries.get(group)?.now.as_deref()
    }
    pub(crate) fn current_node(&self, mode: ProxyMode) -> Option<&str> {
        if mode == ProxyMode::Direct {
            return Some("DIRECT");
        }
        let mut current = if mode == ProxyMode::Global {
            "GLOBAL"
        } else {
            self.order
                .iter()
                .find(|name| name.as_str() != "GLOBAL")
                .map(String::as_str)?
        };
        let mut seen = BTreeSet::new();
        while seen.insert(current) {
            let Some(proxy) = self.entries.get(current) else {
                return Some(current);
            };
            match proxy.now.as_deref() {
                Some(next) => current = next,
                None => return Some(current),
            }
        }
        None
    }
}
#[derive(Clone, Debug)]
pub(crate) enum Action {
    Refresh,
    Select {
        group: String,
        node: String,
        generation: u64,
        epoch: u64,
    },
    Unfix {
        group: String,
        generation: u64,
        epoch: u64,
    },
    Mode(ProxyMode),
}
#[derive(Clone, Debug)]
pub(crate) struct Test {
    pub(crate) group: Option<String>,
    pub(crate) node: Option<String>,
    pub(crate) generation: u64,
    pub(crate) epoch: u64,
}
#[derive(Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Selections {
    schema_version: u32,
    profiles: BTreeMap<u64, BTreeMap<String, String>>,
}
impl Selections {
    fn path(paths: &AppPaths) -> std::path::PathBuf {
        paths.records().join("proxy-selections.json")
    }
    pub(crate) fn load(paths: &AppPaths) -> Result<Self, String> {
        let path = Self::path(paths);
        let bytes = match std::fs::read(path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Self {
                    schema_version: 1,
                    ..Self::default()
                });
            }
            Err(_) => return Err("无法读取代理选择记录".into()),
        };
        if bytes.len() > 1024 * 1024 {
            return Err("代理选择记录超过上限".into());
        }
        let result: Self = serde_json::from_slice(&bytes).map_err(|_| "代理选择记录无效")?;
        if result.schema_version != 1 {
            return Err("代理选择记录版本无效".into());
        }
        Ok(result)
    }
    pub(crate) fn for_profile(&self, id: Option<u64>) -> BTreeMap<String, String> {
        self.profiles
            .get(&id.unwrap_or(0))
            .cloned()
            .unwrap_or_default()
    }
    pub(crate) fn update(&mut self, id: Option<u64>, group: &str, node: Option<&str>) {
        let values = self.profiles.entry(id.unwrap_or(0)).or_default();
        if let Some(node) = node {
            values.insert(group.into(), node.into());
        } else {
            values.remove(group);
        }
    }
    pub(crate) fn save(&self, paths: &AppPaths) -> Result<(), String> {
        let bytes = serde_json::to_vec_pretty(self).map_err(|_| "无法编码代理选择记录")?;
        if bytes.len() > 1024 * 1024 {
            return Err("代理选择记录超过上限".into());
        }
        atomic_write(&Self::path(paths), &bytes).map_err(|_| "代理选择记录保存失败".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_observed_order_nested_choices_and_cycles() {
        let data=br#"{"proxies":{"a":{"name":"a","type":"Selector","all":["b"],"now":"b"},"b":{"name":"b","type":"URLTest","all":["DIRECT"],"now":"DIRECT","fixed":"DIRECT"},"DIRECT":{"name":"DIRECT","type":"Direct"},"GLOBAL":{"name":"GLOBAL","type":"Selector","all":["a"],"now":"a"}}}"#;
        let mut observed = Proxies::parse(data, &["a".into(), "b".into()]).unwrap();
        assert_eq!(observed.order, vec!["a", "b", "GLOBAL"]);
        assert_eq!(observed.current_node(ProxyMode::Rule), Some("DIRECT"));
        assert_eq!(observed.current_node(ProxyMode::Global), Some("DIRECT"));
        observed.entries.get_mut("b").unwrap().now = Some("a".into());
        assert_eq!(observed.current_node(ProxyMode::Rule), None);
        assert_eq!(observed.current_node(ProxyMode::Direct), Some("DIRECT"));
        assert!(
            Proxies::parse(
                br#"{"proxies":{"a":{"name":"other","type":"Direct"}}}"#,
                &[]
            )
            .is_err()
        );
    }
    #[test]
    fn selections_are_isolated_by_profile_and_survive_reload() {
        let root = tempfile::tempdir().unwrap();
        let paths = AppPaths::isolated(root.path());
        std::fs::create_dir_all(paths.records()).unwrap();
        let mut selections = Selections::load(&paths).unwrap();
        selections.update(Some(1), "group", Some("a"));
        selections.update(Some(2), "group", Some("b"));
        selections.save(&paths).unwrap();
        let mut loaded = Selections::load(&paths).unwrap();
        assert_eq!(
            loaded.for_profile(Some(1)).get("group").map(String::as_str),
            Some("a")
        );
        loaded.update(Some(1), "group", None);
        assert!(loaded.for_profile(Some(1)).is_empty());
        assert_eq!(
            loaded.for_profile(Some(2)).get("group").map(String::as_str),
            Some("b")
        );
    }
}

#[cfg(test)]
mod reference_tests {
    use super::Proxies;
    #[test]
    fn group_urls_provider_identity_and_preset_nodes_are_distinct() {
        let mut proxies=Proxies::parse(br#"{"proxies":{"a":{"name":"a","type":"Selector","all":["n","DIRECT"],"testUrl":"http://one.invalid"},"b":{"name":"b","type":"Selector","all":["n"]},"DIRECT":{"name":"DIRECT","type":"Direct"}}}"#,&[]).unwrap();
        proxies
            .include_providers(
                br#"{"providers":{"p / name":{"proxies":[{"name":"n","type":"Trojan"}]}}}"#,
            )
            .unwrap();
        assert_eq!(proxies.entries["n"].provider.as_deref(), Some("p / name"));
        assert_eq!(
            proxies.test_url("a", "http://default.invalid"),
            "http://one.invalid"
        );
        assert_eq!(
            proxies.test_url("b", "http://default.invalid"),
            "http://default.invalid"
        );
        assert!(proxies.testable("a", "n"));
        assert!(!proxies.testable("a", "DIRECT"));
        assert!(!proxies.testable("a", "missing"));
        proxies
            .measurements
            .insert(("a".into(), "n".into()), Ok(20));
        proxies
            .measurements
            .insert(("b".into(), "n".into()), Err("请求超时".into()));
        assert_ne!(
            proxies.measurements[&("a".into(), "n".into())],
            proxies.measurements[&("b".into(), "n".into())]
        );
    }
}
