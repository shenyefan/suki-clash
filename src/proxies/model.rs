#[derive(Clone)]
pub(super) struct Node {
    id: String,
    name: String,
    region: String,
    protocol: String,
    delay: Option<u32>,
    tested: bool,
    failure: Option<String>,
    testing: bool,
}

impl Node {
    pub(super) fn id(&self) -> String {
        self.id.clone()
    }
    pub(super) fn name(&self) -> String {
        self.name.clone()
    }
    pub(super) fn region(&self) -> String {
        self.region.clone()
    }
    pub(super) fn protocol(&self) -> String {
        self.protocol.clone()
    }
    pub(super) fn delay(&self) -> Option<u32> {
        self.delay
    }

    pub(super) fn testing(&self) -> bool {
        self.testing
    }
    pub(super) fn failure(&self) -> Option<&str> {
        self.failure.as_deref()
    }
    pub(super) fn testable(&self) -> bool {
        !matches!(
            self.name.as_str(),
            "DIRECT" | "REJECT" | "REJECT-DROP" | "PASS" | "COMPATIBLE"
        ) && self.protocol != "未知"
    }
    pub(super) fn tested(&self) -> bool {
        self.tested
    }
    fn matches(&self, query: &str) -> bool {
        [&self.name, &self.region, &self.protocol]
            .iter()
            .any(|value| value.to_lowercase().contains(query))
    }
}

#[derive(Clone, Copy, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(super) enum GroupKind {
    Select,
    UrlTest,
    Media,
    Fallback,
    LoadBalance,
    Relay,
}

impl GroupKind {
    pub(super) fn label(self) -> &'static str {
        match self {
            Self::Select => "手动选择",
            Self::UrlTest => "延迟优选",
            Self::Media => "手动选择",
            Self::Fallback => "故障转移",
            Self::LoadBalance => "负载均衡",
            Self::Relay => "链式代理",
        }
    }
}

pub(super) struct ProxyGroup {
    id: String,
    name: String,
    kind: GroupKind,
    nodes: Vec<Node>,
}

impl ProxyGroup {
    pub(super) fn observed(
        proxies: &crate::proxies::runtime::Proxies,
        mode: Option<crate::session::ProxyMode>,
        timeout: u32,
    ) -> Vec<Self> {
        proxies
            .order
            .iter()
            .filter(|name| {
                if mode == Some(crate::session::ProxyMode::Global) {
                    name.as_str() == "GLOBAL"
                } else {
                    name.as_str() != "GLOBAL"
                }
            })
            .filter_map(|name| {
                let group = proxies.entries.get(name)?;
                if group.hidden {
                    return None;
                }
                let kind = match group.kind.as_str() {
                    "Selector" => GroupKind::Select,
                    "URLTest" => GroupKind::UrlTest,
                    "Fallback" => GroupKind::Fallback,
                    "LoadBalance" => GroupKind::LoadBalance,
                    "Relay" => GroupKind::Relay,
                    _ => return None,
                };
                Some(Self {
                    id: name.clone(),
                    name: name.clone(),
                    kind,
                    nodes: group
                        .all
                        .as_ref()?
                        .iter()
                        .map(|name| {
                            let proxy = proxies.entries.get(name);
                            Node {
                                id: name.clone(),
                                name: name.clone(),
                                region: String::new(),
                                protocol: proxy
                                    .map(|proxy| proxy.kind.clone())
                                    .unwrap_or_else(|| "未知".into()),
                                delay: proxies
                                    .measurements
                                    .get(&(group.name.clone(), name.clone()))
                                    .map(|result| result.as_ref().copied().unwrap_or(0))
                                    .or_else(|| {
                                        proxy
                                            .and_then(|proxy| proxy.history.last())
                                            .map(|history| history.delay)
                                    })
                                    .filter(|delay| *delay > 0 && *delay < timeout),
                                testing: proxies
                                    .testing
                                    .contains(&(group.name.clone(), name.clone())),
                                failure: proxies
                                    .measurements
                                    .get(&(group.name.clone(), name.clone()))
                                    .and_then(|result| result.as_ref().err())
                                    .cloned(),
                                tested: proxies
                                    .measurements
                                    .contains_key(&(group.name.clone(), name.clone()))
                                    || proxy.is_some_and(|proxy| !proxy.history.is_empty()),
                            }
                        })
                        .collect(),
                })
            })
            .collect()
    }
    pub(super) fn len(&self) -> usize {
        self.nodes.len()
    }
    pub(super) fn id(&self) -> String {
        self.id.clone()
    }
    pub(super) fn name(&self) -> String {
        self.name.clone()
    }
    pub(super) fn kind(&self) -> GroupKind {
        self.kind
    }
    pub(super) fn default_node(&self) -> String {
        "—".into()
    }

    pub(super) fn visible_nodes(
        &self,
        query: &str,
        available: bool,
        sort: SortOrder,
        reverse: bool,
    ) -> Vec<Node> {
        let matches_group = self.name.to_lowercase().contains(query);
        let mut nodes: Vec<_> = self
            .nodes
            .iter()
            .filter(|node| {
                (matches_group || node.matches(query)) && (!available || node.delay.is_some())
            })
            .cloned()
            .collect();
        match sort {
            SortOrder::Default => {
                if reverse {
                    nodes.reverse();
                }
            }
            SortOrder::Name => nodes.sort_by(|a, b| {
                if reverse {
                    b.name.cmp(&a.name)
                } else {
                    a.name.cmp(&b.name)
                }
            }),
            SortOrder::Latency => nodes.sort_by(|a, b| {
                let rank = |node: &Node| {
                    if node.testing {
                        3
                    } else if node.delay.is_some() {
                        0
                    } else if node.failure.is_some() {
                        2
                    } else if node.tested {
                        1
                    } else {
                        4
                    }
                };
                rank(a)
                    .cmp(&rank(b))
                    .then_with(|| match (a.delay, b.delay) {
                        (Some(a), Some(b)) => {
                            if reverse {
                                b.cmp(&a)
                            } else {
                                a.cmp(&b)
                            }
                        }
                        _ => std::cmp::Ordering::Equal,
                    })
            }),
        }
        nodes
    }
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub(super) enum SortOrder {
    #[default]
    Default,
    Latency,
    Name,
}

impl SortOrder {
    pub(super) const ALL: [Self; 3] = [Self::Default, Self::Latency, Self::Name];
    pub(super) fn label(self) -> &'static str {
        match self {
            Self::Default => "配置顺序",
            Self::Latency => "按延迟",
            Self::Name => "按名称",
        }
    }
}

/// Explicit membership: groups may share a node, while each group owns its
/// selection and disclosure state. No positional IDs are used by the UI.
#[cfg(test)]
pub(super) fn sample_groups() -> Vec<ProxyGroup> {
    let nodes: Vec<Node> = [
        ("hk01", "香港 01 · 专线", "HK", "Trojan", Some(28)),
        ("hk02", "香港 02 · 专线", "HK", "Trojan", Some(35)),
        ("jp01", "日本 01 · 东京", "JP", "Shadowsocks", Some(62)),
        ("jp02", "日本 02 · 大阪", "JP", "Shadowsocks", Some(71)),
        ("sg01", "新加坡 01 · 优选", "SG", "VLESS", Some(84)),
        ("sg02", "新加坡 02 · 优选", "SG", "VLESS", Some(92)),
        ("us01", "美国 01 · 洛杉矶", "US", "Trojan", Some(168)),
        ("us02", "美国 02 · 西雅图", "US", "Trojan", None),
    ]
    .into_iter()
    .map(|(id, name, region, protocol, delay)| Node {
        id: id.into(),
        name: name.into(),
        region: region.into(),
        protocol: protocol.into(),
        delay,
        tested: true,
        failure: None,
        testing: false,
    })
    .collect();
    vec![
        ProxyGroup {
            id: "proxy".into(),
            name: "节点选择".into(),
            kind: GroupKind::Select,
            nodes: nodes.clone(),
        },
        ProxyGroup {
            id: "auto".into(),
            name: "自动选择".into(),
            kind: GroupKind::UrlTest,
            nodes: nodes
                .iter()
                .filter(|node| node.region != "US")
                .cloned()
                .collect(),
        },
        ProxyGroup {
            id: "media".into(),
            name: "流媒体".into(),
            kind: GroupKind::Media,
            nodes: nodes
                .into_iter()
                .filter(|node| ["hk01", "jp01", "sg01", "us01"].contains(&node.id.as_str()))
                .collect(),
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn latency_sort_keeps_unreachable_nodes_last_in_both_directions() {
        let groups = sample_groups();
        for reverse in [false, true] {
            let nodes = groups[0].visible_nodes("", false, SortOrder::Latency, reverse);
            assert_eq!(nodes.last().unwrap().id(), "us02");
            assert_eq!(nodes[0].id(), if reverse { "us01" } else { "hk01" });
        }
    }

    #[test]
    fn filtering_preserves_group_membership_and_configuration_order() {
        let groups = sample_groups();
        assert!(
            groups[1]
                .visible_nodes("us", false, SortOrder::Default, false)
                .is_empty()
        );
        let nodes = groups[2].visible_nodes("jp", false, SortOrder::Default, false);
        assert_eq!(
            nodes.iter().map(|node| node.id()).collect::<Vec<_>>(),
            ["jp01"]
        );
        assert_eq!(
            groups[0]
                .visible_nodes("节点选择", true, SortOrder::Default, false)
                .len(),
            7
        );
    }
}

#[cfg(test)]
mod delay_order_tests {
    #[test]
    fn timeout_error_testing_and_untested_follow_measurements_in_both_directions() {
        let mut group = super::sample_groups().remove(0);
        let mut nodes = group.nodes[..5].to_vec();
        for (index, node) in nodes.iter_mut().enumerate() {
            node.id = index.to_string();
            node.delay = None;
            node.testing = false;
            node.failure = None;
            node.tested = false;
        }
        nodes[1].testing = true;
        nodes[2].tested = true;
        nodes[2].failure = Some("连接失败".into());
        nodes[3].tested = true;
        nodes[4].tested = true;
        nodes[4].delay = Some(25);
        group.nodes = nodes;
        for reverse in [false, true] {
            assert_eq!(
                group
                    .visible_nodes("", false, super::SortOrder::Latency, reverse)
                    .iter()
                    .map(|node| node.id())
                    .collect::<Vec<_>>(),
                ["4", "3", "2", "1", "0"]
            );
        }
    }
}
