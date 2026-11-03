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
    #[serde(default)]
    pub(crate) udp: bool,
    #[serde(default)]
    pub(crate) xudp: bool,
    #[serde(default)]
    pub(crate) tfo: bool,
    #[serde(default)]
    pub(crate) mptcp: bool,
    #[serde(default)]
    pub(crate) smux: bool,
    #[serde(skip)]
    pub(crate) provider: Option<String>,
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
pub(crate) struct Delay {
    pub(crate) delay: u32,
}
/// Measurement failures retain their protocol meaning independently of display text.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) enum MeasurementError {
    Timeout,
    Failed(String),
}
impl std::fmt::Display for MeasurementError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Timeout => formatter.write_str("测速超时"),
            Self::Failed(message) => formatter.write_str(message),
        }
    }
}
impl From<String> for MeasurementError {
    fn from(message: String) -> Self {
        Self::Failed(message)
    }
}
impl From<&str> for MeasurementError {
    fn from(message: &str) -> Self {
        Self::Failed(message.to_owned())
    }
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct GroupScope {
    pub(crate) providers: Vec<String>,
    pub(crate) exclude_types: Vec<String>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Member {
    pub(crate) name: String,
    pub(crate) occurrence: usize,
    pub(crate) record_id: Option<String>,
    pub(crate) reason: Option<&'static str>,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Proxies {
    pub(crate) entries: BTreeMap<String, Proxy>,
    pub(crate) order: Vec<String>,
    pub(crate) records: BTreeMap<String, Proxy>,
    pub(crate) members: BTreeMap<String, Vec<Member>>,
    pub(crate) scopes: BTreeMap<String, GroupScope>,
    pub(crate) provider_error: Option<String>,
    provider_names: BTreeMap<String, Vec<String>>,
    core_names: BTreeSet<String>,
    pub(crate) measurements: BTreeMap<(String, String), Result<u32, MeasurementError>>,
    pub(crate) testing: BTreeSet<(String, String)>,
    pub(crate) batches: BTreeSet<(String, String)>,
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
            batches: Default::default(),
            records: Default::default(),
            members: Default::default(),
            scopes: Default::default(),
            provider_error: None,
            provider_names: Default::default(),
            core_names: Default::default(),
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
        result.core_names = result.entries.keys().cloned().collect();
        for (name, proxy) in &result.entries {
            result
                .records
                .insert(format!("c:{}:{name}", name.len()), proxy.clone());
        }
        result.resolve_members();
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
        self.entries
            .retain(|name, _| self.core_names.contains(name));
        self.records.retain(|_, proxy| proxy.provider.is_none());
        self.provider_names.clear();
        for (provider, data) in response.providers {
            let mut occurrences = BTreeMap::<String, usize>::new();
            for mut proxy in data.proxies {
                if proxy.name.is_empty() || proxy.name.len() > 1024 {
                    return Err("代理集合节点身份无效".into());
                }
                let occurrence = occurrences.entry(proxy.name.clone()).or_default();
                let id = format!(
                    "p:{}:{}:{}:{}:{}",
                    provider.len(),
                    provider,
                    proxy.name.len(),
                    proxy.name,
                    occurrence
                );
                *occurrence += 1;
                proxy.provider = Some(provider.clone());
                self.provider_names
                    .entry(proxy.name.clone())
                    .or_default()
                    .push(id.clone());
                self.records.insert(id, proxy);
                if self.records.len() > 20000 {
                    return Err("代理列表超过上限".into());
                }
            }
        }
        for (name, ids) in &self.provider_names {
            if !self.core_names.contains(name) && ids.len() == 1 {
                self.entries
                    .insert(name.clone(), self.records[&ids[0]].clone());
            }
        }
        self.provider_error = None;
        self.resolve_members();
        Ok(())
    }
    pub(crate) fn resolve_members(&mut self) {
        self.members.clear();
        for group in &self.order {
            let mut counts = BTreeMap::<String, usize>::new();
            let members = self
                .entries
                .get(group)
                .and_then(|proxy| proxy.all.as_ref())
                .into_iter()
                .flatten()
                .map(|name| {
                    let occurrence = counts.entry(name.clone()).or_default();
                    let nth = *occurrence;
                    *occurrence += 1;
                    let mut candidates = vec![];
                    if let Some(scope) = self.scopes.get(group) {
                        for provider in &scope.providers {
                            for id in self.provider_names.get(name).into_iter().flatten() {
                                let proxy = &self.records[id];
                                if proxy.provider.as_ref() == Some(provider)
                                    && !scope
                                        .exclude_types
                                        .iter()
                                        .any(|kind| kind.eq_ignore_ascii_case(&proxy.kind))
                                {
                                    candidates.push(id.clone());
                                }
                            }
                        }
                    }
                    let (record_id, reason) = if self.core_names.contains(name) {
                        (Some(format!("c:{}:{name}", name.len())), None)
                    } else if let Some(id) = candidates.get(nth).or_else(|| candidates.first()) {
                        (Some(id.clone()), None)
                    } else {
                        match self
                            .provider_names
                            .get(name)
                            .map(Vec::as_slice)
                            .unwrap_or_default()
                        {
                            [id] => (Some(id.clone()), None),
                            [] => (
                                None,
                                Some(if self.provider_error.is_some() {
                                    "代理集合不可用"
                                } else {
                                    "节点尚未解析"
                                }),
                            ),
                            _ => (None, Some("同名节点来源不明确")),
                        }
                    };
                    Member {
                        name: name.clone(),
                        occurrence: nth,
                        record_id,
                        reason,
                    }
                })
                .collect();
            self.members.insert(group.clone(), members);
        }
    }
    pub(crate) fn member_proxy(&self, member: &Member) -> Option<&Proxy> {
        member
            .record_id
            .as_ref()
            .and_then(|id| self.records.get(id))
    }
    /// Follow actual selected members through their resolved source records.
    /// Duplicate names that point at distinct records are not guessed from a global name map.
    pub(crate) fn selected_path<'a>(
        &'a self,
        member: &'a Member,
    ) -> Result<Vec<&'a Proxy>, &'static str> {
        let mut proxy = self
            .member_proxy(member)
            .ok_or(member.reason.unwrap_or("节点尚未解析"))?;
        let mut seen = BTreeSet::new();
        let mut path = Vec::new();
        loop {
            if !seen.insert(proxy.name.as_str()) {
                return Err("代理组选择存在循环");
            }
            path.push(proxy);
            if proxy.all.is_none() {
                return Ok(path);
            }
            let now = proxy.now.as_deref().ok_or("代理组尚未选择节点")?;
            let candidates = self
                .members
                .get(&proxy.name)
                .into_iter()
                .flatten()
                .filter(|member| member.name == now)
                .collect::<Vec<_>>();
            let first = candidates.first().ok_or("当前节点尚未解析")?;
            if candidates
                .iter()
                .any(|candidate| candidate.record_id != first.record_id)
            {
                return Err("当前同名节点来源不明确");
            }
            proxy = self
                .member_proxy(first)
                .ok_or(first.reason.unwrap_or("当前节点尚未解析"))?;
        }
    }
    pub(crate) fn measurement_key(&self, group: &str, member: &Member) -> String {
        if self.members.get(group).is_some_and(|members| {
            members
                .iter()
                .filter(|item| item.name == member.name)
                .count()
                > 1
        }) {
            member
                .record_id
                .clone()
                .unwrap_or_else(|| format!("u:{}:{}", member.name, member.occurrence))
        } else {
            member.name.clone()
        }
    }
    pub(crate) fn selectable(&self, group: &str, node: &str) -> bool {
        self.entries
            .get(group)
            .is_some_and(|proxy| matches!(proxy.kind.as_str(), "Selector" | "URLTest" | "Fallback"))
            && self.members.get(group).is_some_and(|members| {
                members
                    .iter()
                    .any(|member| member.name == node && self.member_proxy(member).is_some())
            })
    }
    pub(crate) fn testable(&self, group: &str, node: &str) -> bool {
        !matches!(
            node,
            "DIRECT" | "REJECT" | "REJECT-DROP" | "PASS" | "COMPATIBLE"
        ) && self.members.get(group).is_some_and(|members| {
            members
                .iter()
                .any(|member| member.name == node && self.member_proxy(member).is_some())
        })
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
    pub(crate) fn current_node_in<'a>(&'a self, mut current: &'a str) -> Option<&'a str> {
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
    pub(crate) urls: BTreeMap<String, String>,
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
    fn selected_path_stops_at_cycles_and_does_not_guess_unresolved_members() {
        let proxies = Proxies::parse(br#"{"proxies":{"a":{"name":"a","type":"Selector","all":["b"],"now":"b"},"b":{"name":"b","type":"Selector","all":["a"],"now":"a"}}}"#, &[]).unwrap();
        assert_eq!(
            proxies.selected_path(&proxies.members["a"][0]).unwrap_err(),
            "代理组选择存在循环"
        );
        let proxies = Proxies::parse(br#"{"proxies":{"a":{"name":"a","type":"Selector","all":["missing"],"now":"missing"}}}"#, &[]).unwrap();
        assert_eq!(
            proxies.selected_path(&proxies.members["a"][0]).unwrap_err(),
            "节点尚未解析"
        );
    }
    #[test]
    fn parses_observed_order_nested_choices_and_cycles() {
        let data=br#"{"proxies":{"a":{"name":"a","type":"Selector","all":["b"],"now":"b"},"b":{"name":"b","type":"URLTest","all":["DIRECT"],"now":"DIRECT","fixed":"DIRECT"},"DIRECT":{"name":"DIRECT","type":"Direct"},"GLOBAL":{"name":"GLOBAL","type":"Selector","all":["a"],"now":"a"}}}"#;
        let mut observed = Proxies::parse(data, &["a".into(), "b".into()]).unwrap();
        assert_eq!(observed.order, vec!["a", "b", "GLOBAL"]);
        assert_eq!(observed.current_node_in("a"), Some("DIRECT"));
        assert_eq!(observed.current_node_in("GLOBAL"), Some("DIRECT"));
        observed.entries.get_mut("b").unwrap().now = Some("a".into());
        assert_eq!(observed.current_node_in("a"), None);
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

#[cfg(test)]
mod source_identity_tests {
    use super::*;
    #[test]
    fn selection_requires_a_resolved_member_and_preserves_preset_selection() {
        let mut proxies = Proxies::parse(br#"{"proxies":{"g":{"name":"g","type":"Selector","all":["DIRECT","n","unknown"]},"DIRECT":{"name":"DIRECT","type":"Direct"},"balanced":{"name":"balanced","type":"LoadBalance","all":["DIRECT"]}}}"#, &[]).unwrap();
        assert!(proxies.selectable("g", "DIRECT"));
        assert!(!proxies.testable("g", "DIRECT"));
        assert!(!proxies.selectable("g", "n"));
        assert!(!proxies.selectable("g", "unknown"));
        assert!(!proxies.selectable("balanced", "DIRECT"));
        proxies
            .include_providers(br#"{"providers":{"p":{"proxies":[{"name":"n","type":"Trojan"}]}}}"#)
            .unwrap();
        assert!(proxies.selectable("g", "n"));
        assert!(proxies.testable("g", "n"));
        assert!(!proxies.selectable("g", "unknown"));
        proxies.include_providers(br#"{"providers":{}}"#).unwrap();
        assert!(!proxies.selectable("g", "n"));
    }
    #[test]
    fn duplicate_provider_members_follow_use_order_occurrence_and_rebind() {
        let mut proxies = Proxies::parse(
            br#"{"proxies":{"g":{"name":"g","type":"Selector","all":["n","n","missing"]}}}"#,
            &[],
        )
        .unwrap();
        assert!(
            proxies.members["g"]
                .iter()
                .all(|member| member.record_id.is_none())
        );
        proxies.scopes.insert(
            "g".into(),
            GroupScope {
                providers: vec!["b".into(), "a".into()],
                exclude_types: vec![],
            },
        );
        proxies.include_providers(br#"{"providers":{"a":{"proxies":[{"name":"n","type":"Trojan"}]},"b":{"proxies":[{"name":"n","type":"Socks5"}]}}}"#).unwrap();
        let members = &proxies.members["g"];
        assert_eq!(
            proxies
                .member_proxy(&members[0])
                .unwrap()
                .provider
                .as_deref(),
            Some("b")
        );
        assert_eq!(
            proxies
                .member_proxy(&members[1])
                .unwrap()
                .provider
                .as_deref(),
            Some("a")
        );
        assert_ne!(
            proxies.measurement_key("g", &members[0]),
            proxies.measurement_key("g", &members[1])
        );
        assert_eq!(members[2].reason, Some("节点尚未解析"));
        proxies
            .scopes
            .get_mut("g")
            .unwrap()
            .exclude_types
            .push("socks5".into());
        proxies.resolve_members();
        assert!(
            proxies.members["g"].iter().take(2).all(|member| proxies
                .member_proxy(member)
                .unwrap()
                .provider
                .as_deref()
                == Some("a"))
        );
        let mut core=Proxies::parse(br#"{"proxies":{"g":{"name":"g","type":"Selector","all":["n"]},"n":{"name":"n","type":"Http"}}}"#,&[]).unwrap();
        core.include_providers(
            br#"{"providers":{"p":{"proxies":[{"name":"n","type":"Trojan"}]}}}"#,
        )
        .unwrap();
        assert_eq!(
            core.member_proxy(&core.members["g"][0]).unwrap().kind,
            "Http"
        );
    }
}
