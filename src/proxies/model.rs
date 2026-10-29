#[derive(Default)]
pub(super) struct Filter {
    pub(super) text: crate::search::Search,
    expression: Option<(char, u32, bool)>,
    protocol: Option<String>,
}
impl Filter {
    pub(super) fn compile(&mut self, query: &str) {
        self.expression = None;
        self.protocol = None;
        let query = query.trim();
        if let Ok(pattern) = regex::Regex::new(r"(?i)delay([=<>])(\d+|timeout|error)")
            && let Some(parts) = pattern.captures(query)
        {
            self.expression = Some((
                parts[1].chars().next().unwrap_or('='),
                match parts[2].to_ascii_lowercase().as_str() {
                    "timeout" => 3000,
                    "error" => 100000,
                    value => value.parse().unwrap_or(u32::MAX),
                },
                parts[2].eq_ignore_ascii_case("timeout") || parts[2].eq_ignore_ascii_case("error"),
            ));
        }
        if self.expression.is_none()
            && let Some(index) = query.to_ascii_lowercase().find("type=")
        {
            self.protocol = Some(query[index + 5..].to_ascii_lowercase());
        }
        let whole_word = self.text.whole_word;
        if self.text.regular_expression {
            self.text.whole_word = false;
        }
        self.text
            .compile(if self.expression.is_some() || self.protocol.is_some() {
                ""
            } else {
                query
            });
        self.text.whole_word = whole_word;
    }
    fn matches(&self, node: &Node) -> bool {
        if let Some((operator, threshold, special)) = self.expression {
            let value = node.delay.or_else(|| {
                if node
                    .failure
                    .as_ref()
                    .is_some_and(|failure| !failure.contains("超时"))
                {
                    Some(100000)
                } else if node.tested {
                    Some(3000)
                } else {
                    None
                }
            });
            return value.is_some_and(|value| match operator {
                '=' if special && threshold == 3000 => (3000..100000).contains(&value),
                '=' if special && threshold == 100000 => value >= 100000,
                '=' => value == threshold,
                '<' => value <= threshold,
                _ => value >= threshold,
            });
        }
        if let Some(protocol) = &self.protocol {
            return node.protocol.to_ascii_lowercase().contains(protocol);
        }
        self.text.error.is_none() && self.text.matches(&node.name)
    }
}
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
    sort_key: Option<(u8, Option<u32>)>,
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
    fn latency_key(&self) -> (u8, Option<u32>) {
        self.sort_key.unwrap_or_else(|| {
            (
                if self.testing {
                    3
                } else if self.delay.is_some() {
                    0
                } else if self.failure.is_some() {
                    2
                } else if self.tested {
                    1
                } else {
                    4
                },
                self.delay,
            )
        })
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
                    nodes: proxies
                        .members
                        .get(name)?
                        .iter()
                        .map(|member| {
                            let name = &member.name;
                            let proxy = proxies.member_proxy(member);
                            let measurement_key = proxies.measurement_key(&group.name, member);
                            Node {
                                id: format!(
                                    "{}:{}",
                                    member.record_id.as_deref().unwrap_or(name),
                                    member.occurrence
                                ),
                                name: name.clone(),
                                region: String::new(),
                                protocol: proxy
                                    .map(|proxy| proxy.kind.clone())
                                    .unwrap_or_else(|| "未知".into()),
                                delay: proxies
                                    .measurements
                                    .get(&(group.name.clone(), measurement_key.clone()))
                                    .map(|result| result.as_ref().copied().unwrap_or(0))
                                    .or_else(|| {
                                        proxy
                                            .and_then(|proxy| proxy.history.last())
                                            .map(|history| history.delay)
                                    })
                                    .filter(|delay| *delay > 0 && *delay < timeout),
                                sort_key: None,
                                testing: proxies
                                    .testing
                                    .contains(&(group.name.clone(), measurement_key.clone())),
                                failure: proxies
                                    .measurements
                                    .get(&(group.name.clone(), measurement_key.clone()))
                                    .and_then(|result| result.as_ref().err())
                                    .cloned()
                                    .or_else(|| member.reason.map(str::to_owned)),
                                tested: proxies
                                    .measurements
                                    .contains_key(&(group.name.clone(), measurement_key.clone()))
                                    || proxy.is_some_and(|proxy| !proxy.history.is_empty()),
                            }
                        })
                        .collect(),
                })
            })
            .collect()
    }
    pub(super) fn keep_latency_order(&mut self, previous: &Self) {
        for node in &mut self.nodes {
            if let Some(old) = previous.nodes.iter().find(|old| old.id == node.id) {
                node.sort_key = Some(old.latency_key());
            }
        }
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

    pub(super) fn visible_filtered(
        &self,
        filter: &Filter,
        available: bool,
        sort: SortOrder,
        reverse: bool,
    ) -> Vec<Node> {
        self.visible_nodes("", available, sort, reverse)
            .into_iter()
            .filter(|node| filter.matches(node))
            .collect()
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
                let (a_rank, a_delay) = a.latency_key();
                let (b_rank, b_delay) = b.latency_key();
                a_rank.cmp(&b_rank).then_with(|| match (a_delay, b_delay) {
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
        sort_key: None,
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

#[cfg(test)]
mod incremental_delay_tests {
    use super::*;
    #[test]
    fn measurement_changes_display_without_reordering_until_batch_completion() {
        let previous = sample_groups().remove(0);
        let before = previous
            .visible_nodes("", false, SortOrder::Latency, false)
            .iter()
            .map(Node::id)
            .collect::<Vec<_>>();
        let mut live = sample_groups().remove(0);
        live.nodes[0].delay = Some(900);
        live.nodes[1].testing = true;
        live.keep_latency_order(&previous);
        assert_eq!(live.nodes[0].delay(), Some(900));
        assert_eq!(
            live.visible_nodes("", false, SortOrder::Latency, false)
                .iter()
                .map(Node::id)
                .collect::<Vec<_>>(),
            before
        );
        for node in &mut live.nodes {
            node.sort_key = None;
        }
        assert_ne!(
            live.visible_nodes("", false, SortOrder::Latency, false)
                .iter()
                .map(Node::id)
                .collect::<Vec<_>>(),
            before
        );
    }
}

#[cfg(test)]
mod advanced_search_tests {
    use super::*;
    fn node(name: &str, delay: Option<u32>, failure: Option<&str>) -> Node {
        Node {
            id: name.into(),
            name: name.into(),
            region: String::new(),
            protocol: "Trojan".into(),
            delay,
            tested: delay.is_some() || failure.is_some(),
            failure: failure.map(str::to_owned),
            testing: false,
            sort_key: None,
        }
    }
    #[test]
    fn expressions_preserve_numeric_equality_timeout_ranges_and_invalid_regex() {
        let mut filter = Filter::default();
        let slow = node("Tokyo", Some(3200), None);
        let exact = node("Tokyo", Some(3000), None);
        filter.compile("delay=3000");
        assert!(!filter.matches(&slow));
        assert!(filter.matches(&exact));
        filter.compile("delay=timeout");
        assert!(filter.matches(&slow));
        assert!(!filter.matches(&node("failed", None, Some("连接失败"))));
        filter.compile("delay=error");
        assert!(filter.matches(&node("failed", None, Some("连接失败"))));
        filter.compile("delay<3000");
        assert!(filter.matches(&exact));
        filter.compile("type=troj");
        assert!(filter.matches(&exact));
        filter.text.regular_expression = true;
        filter.text.whole_word = true;
        filter.compile("Tok");
        assert!(filter.matches(&exact));
        filter.compile("[");
        assert!(filter.text.error.is_some());
        assert!(!filter.matches(&node("[", None, None)));
        filter.text.regular_expression = false;
        filter.text.whole_word = false;
        filter.text.case_sensitive = true;
        filter.compile("tokyo");
        assert!(!filter.matches(&exact));
    }
}
