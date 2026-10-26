use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum ProxyMode {
    Rule,
    Global,
    Direct,
}

#[derive(Clone, serde::Deserialize)]
struct ActiveProfile {
    id: u64,
    name: String,
}

/// Read-only projection of observed runtime state shared by the feature views.
#[derive(Default)]
pub(crate) struct Session {
    system_proxy: Option<bool>,
    tun: Option<bool>,
    running: bool,
    mode: Option<ProxyMode>,
    profile: Option<ActiveProfile>,
    nodes: BTreeMap<String, String>,
    fixed_groups: BTreeSet<String>,
}
impl Session {
    pub(crate) fn system_proxy(&self) -> Option<bool> {
        self.system_proxy
    }
    pub(crate) fn tun(&self) -> Option<bool> {
        self.tun
    }
    pub(crate) fn is_running(&self) -> bool {
        self.running
    }
    pub(crate) fn mode(&self) -> Option<ProxyMode> {
        self.mode
    }
    pub(crate) fn active_profile_id(&self) -> Option<u64> {
        self.profile.as_ref().map(|p| p.id)
    }
    pub(crate) fn profile_name(&self) -> Option<&str> {
        self.profile.as_ref().map(|p| p.name.as_str())
    }
    pub(crate) fn selected_node(&self, group: &str) -> Option<&str> {
        self.nodes.get(group).map(String::as_str)
    }
    pub(crate) fn is_fixed(&self, group: &str) -> bool {
        self.fixed_groups.contains(group)
    }
}
