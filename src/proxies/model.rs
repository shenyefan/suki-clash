#[derive(Clone, Copy)]
pub(super) struct Node {
    id: &'static str,
    name: &'static str,
    region: &'static str,
    protocol: &'static str,
    delay: Option<u32>,
}

impl Node {
    pub(super) fn id(self) -> &'static str {
        self.id
    }
    pub(super) fn name(self) -> &'static str {
        self.name
    }
    pub(super) fn region(self) -> &'static str {
        self.region
    }
    pub(super) fn protocol(self) -> &'static str {
        self.protocol
    }
    pub(super) fn delay(self) -> Option<u32> {
        self.delay
    }

    fn matches(self, query: &str) -> bool {
        [self.name, self.region, self.protocol]
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
}

impl GroupKind {
    pub(super) fn label(self) -> &'static str {
        match self {
            Self::Select => "手动选择",
            Self::UrlTest => "延迟优选",
            Self::Media => "手动选择",
        }
    }
}

pub(super) struct ProxyGroup {
    id: &'static str,
    name: &'static str,
    kind: GroupKind,
    nodes: Vec<Node>,
}

impl ProxyGroup {
    pub(super) fn id(&self) -> &'static str {
        self.id
    }
    pub(super) fn name(&self) -> &'static str {
        self.name
    }
    pub(super) fn kind(&self) -> GroupKind {
        self.kind
    }
    pub(super) fn default_node(&self) -> &'static str {
        self.nodes.first().map(|node| node.name()).unwrap_or("—")
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
            .copied()
            .filter(|node| {
                (matches_group || node.matches(query)) && (!available || node.delay.is_some())
            })
            .collect();
        match sort {
            SortOrder::Default => {
                if reverse {
                    nodes.reverse();
                }
            }
            SortOrder::Name => nodes.sort_by(|a, b| {
                if reverse {
                    b.name.cmp(a.name)
                } else {
                    a.name.cmp(b.name)
                }
            }),
            SortOrder::Latency => nodes.sort_by(|a, b| match (a.delay, b.delay) {
                (Some(a), Some(b)) => {
                    if reverse {
                        b.cmp(&a)
                    } else {
                        a.cmp(&b)
                    }
                }
                (Some(_), None) => std::cmp::Ordering::Less,
                (None, Some(_)) => std::cmp::Ordering::Greater,
                (None, None) => std::cmp::Ordering::Equal,
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
        id,
        name,
        region,
        protocol,
        delay,
    })
    .collect();
    vec![
        ProxyGroup {
            id: "proxy",
            name: "节点选择",
            kind: GroupKind::Select,
            nodes: nodes.clone(),
        },
        ProxyGroup {
            id: "auto",
            name: "自动选择",
            kind: GroupKind::UrlTest,
            nodes: nodes
                .iter()
                .copied()
                .filter(|node| node.region != "US")
                .collect(),
        },
        ProxyGroup {
            id: "media",
            name: "流媒体",
            kind: GroupKind::Media,
            nodes: nodes
                .into_iter()
                .filter(|node| ["hk01", "jp01", "sg01", "us01"].contains(&node.id))
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
