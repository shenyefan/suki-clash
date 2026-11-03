//! Shared proxy testing choices. Only the lifecycle actor writes this settings section.
use super::runtime::Proxies;
use crate::{session::ProxyMode, storage::AppPaths};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Preferences {
    pub(crate) profiles: BTreeMap<u64, Profile>,
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Profile {
    pub(crate) dashboard_group: Option<String>,
    pub(crate) test_urls: BTreeMap<String, String>,
}
#[derive(Clone)]
pub(crate) enum Change {
    DashboardGroup(String),
    TestUrl { group: String, url: String },
}
impl Preferences {
    pub(crate) fn load(paths: &AppPaths) -> Result<Self, String> {
        crate::preferences::app::read(paths, "proxies")
    }
    pub(crate) fn save(&self, paths: &AppPaths) -> Result<(), String> {
        crate::preferences::app::write(paths, "proxies", self)
    }
    pub(crate) fn profile(&self, id: Option<u64>) -> Profile {
        self.profiles
            .get(&id.unwrap_or(0))
            .cloned()
            .unwrap_or_default()
    }
    pub(crate) fn update(&mut self, id: Option<u64>, change: Change) -> Result<(), String> {
        let profile = self.profiles.entry(id.unwrap_or(0)).or_default();
        match change {
            Change::DashboardGroup(group) => profile.dashboard_group = Some(group),
            Change::TestUrl { group, url } => {
                let url = url.trim();
                if !url.is_empty() && !crate::profiles::model::valid_remote_url(url) {
                    return Err("代理组测速地址无效".into());
                }
                if url.is_empty() {
                    profile.test_urls.remove(&group);
                } else {
                    profile.test_urls.insert(group, url.to_owned());
                }
            }
        }
        Ok(())
    }
}
impl Profile {
    pub(crate) fn group<'a>(&self, proxies: &'a Proxies, mode: ProxyMode) -> Option<&'a str> {
        if mode == ProxyMode::Direct {
            return None;
        }
        if mode == ProxyMode::Global {
            return proxies.entries.contains_key("GLOBAL").then_some("GLOBAL");
        }
        let eligible = |name: &&String| {
            proxies
                .entries
                .get(*name)
                .is_some_and(|p| !p.hidden && matches!(p.kind.as_str(), "Selector" | "URLTest"))
        };
        let groups = proxies
            .order
            .iter()
            .filter(eligible)
            .filter(|name| name.as_str() != "GLOBAL")
            .collect::<Vec<_>>();
        groups
            .iter()
            .find(|name| self.dashboard_group.as_ref() == Some(**name))
            .or_else(|| {
                groups.iter().find(|name| {
                    ["auto", "select", "proxy", "节点选择", "自动选择"]
                        .iter()
                        .any(|word| name.to_lowercase().contains(word))
                })
            })
            .or_else(|| groups.first())
            .map(|name| name.as_str())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn group_choice_uses_profile_memory_and_explicit_modes() {
        let proxies = Proxies::parse(br#"{"proxies":{"first":{"name":"first","type":"Selector","all":["DIRECT"],"now":"DIRECT"},"proxy":{"name":"proxy","type":"Selector","all":["DIRECT"],"now":"DIRECT"},"other":{"name":"other","type":"URLTest","all":["DIRECT"],"now":"DIRECT"},"GLOBAL":{"name":"GLOBAL","type":"Selector","all":["proxy"],"now":"proxy"},"DIRECT":{"name":"DIRECT","type":"Direct"}}}"#, &["first".into(), "proxy".into(), "other".into()]).unwrap();
        let mut profile = Profile::default();
        assert_eq!(profile.group(&proxies, ProxyMode::Rule), Some("proxy"));
        profile.dashboard_group = Some("other".into());
        assert_eq!(profile.group(&proxies, ProxyMode::Rule), Some("other"));
        assert_eq!(profile.group(&proxies, ProxyMode::Global), Some("GLOBAL"));
        assert_eq!(profile.group(&proxies, ProxyMode::Direct), None);
        profile.dashboard_group = Some("removed".into());
        assert_eq!(profile.group(&proxies, ProxyMode::Rule), Some("proxy"));
    }
    #[test]
    fn profile_choices_survive_reload_without_overwriting_other_sections() {
        let root = tempfile::tempdir().unwrap();
        let paths = AppPaths::isolated(root.path());
        std::fs::create_dir_all(paths.settings()).unwrap();
        crate::preferences::app::write(&paths, "other", &true).unwrap();
        let mut prefs = Preferences::default();
        prefs
            .update(Some(1), Change::DashboardGroup("a".into()))
            .unwrap();
        prefs
            .update(
                Some(1),
                Change::TestUrl {
                    group: "a".into(),
                    url: "https://example.test/test".into(),
                },
            )
            .unwrap();
        prefs
            .update(Some(2), Change::DashboardGroup("b".into()))
            .unwrap();
        prefs.save(&paths).unwrap();
        let loaded = Preferences::load(&paths).unwrap();
        assert_eq!(loaded, prefs);
        assert_eq!(
            loaded.profile(Some(2)).dashboard_group.as_deref(),
            Some("b")
        );
        assert!(loaded.profile(Some(2)).test_urls.is_empty());
        assert!(crate::preferences::app::read::<bool>(&paths, "other").unwrap());
        assert!(
            prefs
                .update(
                    Some(1),
                    Change::TestUrl {
                        group: "a".into(),
                        url: "invalid".into()
                    }
                )
                .is_err()
        );
    }
}
