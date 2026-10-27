//! Runtime resource snapshots. Provider names are identities, never filesystem paths.
use crate::storage::{AppPaths, DirectoryKind};
use serde::Deserialize;
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum ProviderKind {
    Proxies,
    Rules,
}
impl ProviderKind {
    pub(super) fn path(self) -> &'static str {
        match self {
            Self::Proxies => "/providers/proxies",
            Self::Rules => "/providers/rules",
        }
    }
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Proxies => "代理集合",
            Self::Rules => "规则集合",
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct DatabaseFile {
    key: &'static str,
    size: Option<u64>,
    modified: Option<String>,
}
impl DatabaseFile {
    pub(crate) fn key(&self) -> &str {
        self.key
    }
    pub(crate) fn size(&self) -> Option<u64> {
        self.size
    }
    pub(crate) fn modified(&self) -> Option<&str> {
        self.modified.as_deref()
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Provider {
    name: String,
    vehicle: String,
    count: u64,
    updated: Option<String>,
    behavior: Option<String>,
    usage: Option<Usage>,
    uncertain: bool,
}
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub(crate) struct Usage {
    #[serde(default)]
    upload: u64,
    #[serde(default)]
    download: u64,
    #[serde(default)]
    total: u64,
    #[serde(default)]
    expire: u64,
}
impl Provider {
    pub(crate) fn name(&self) -> &str {
        &self.name
    }
    pub(crate) fn vehicle(&self) -> &str {
        &self.vehicle
    }
    pub(crate) fn count(&self) -> u64 {
        self.count
    }
    pub(crate) fn updated(&self) -> Option<&str> {
        self.updated.as_deref()
    }
    pub(crate) fn behavior(&self) -> Option<&str> {
        self.behavior.as_deref()
    }
    pub(crate) fn uncertain(&self) -> bool {
        self.uncertain
    }
    pub(crate) fn can_update(&self) -> bool {
        !self.uncertain
            && (self.vehicle.eq_ignore_ascii_case("http")
                || self.vehicle.eq_ignore_ascii_case("file"))
    }
    pub(crate) fn quota(&self) -> Option<String> {
        self.usage.as_ref().map(|usage| {
            let used = usage.upload.saturating_add(usage.download);
            let mut label = format!("{} / {} 字节", used, usage.total);
            if let Ok(expire) = i64::try_from(usage.expire)
                && let Some(expire) =
                    chrono::DateTime::from_timestamp(expire, 0).filter(|_| usage.expire != 0)
            {
                label.push_str(&format!(" · 到期 {}", expire.format("%Y-%m-%d")));
            }
            label
        })
    }
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Resources {
    pub(super) files: Vec<DatabaseFile>,
    pub(super) proxies: Vec<Provider>,
    pub(super) rules: Vec<Provider>,
    pub(super) error: Option<String>,
}
impl Resources {
    pub(crate) fn files(&self) -> &[DatabaseFile] {
        &self.files
    }
    pub(crate) fn providers(&self, kind: ProviderKind) -> &[Provider] {
        match kind {
            ProviderKind::Proxies => &self.proxies,
            ProviderKind::Rules => &self.rules,
        }
    }
    pub(super) fn mark_uncertain(&mut self, kind: ProviderKind, name: &str) {
        let providers = match kind {
            ProviderKind::Proxies => &mut self.proxies,
            ProviderKind::Rules => &mut self.rules,
        };
        if let Some(provider) = providers.iter_mut().find(|provider| provider.name == name) {
            provider.uncertain = true;
        }
    }
    pub(super) fn refresh_files(&mut self, paths: &AppPaths) {
        self.files = crate::configuration::resources::DATABASES
            .into_iter()
            .map(|(key, _, _, file)| {
                let metadata = std::fs::metadata(paths.directory(DirectoryKind::Core).join(file))
                    .ok()
                    .filter(|metadata| metadata.is_file() && metadata.len() > 0);
                let modified = metadata
                    .as_ref()
                    .and_then(|metadata| metadata.modified().ok())
                    .map(|time| {
                        chrono::DateTime::<chrono::Local>::from(time)
                            .format("%Y-%m-%d %H:%M")
                            .to_string()
                    });
                DatabaseFile {
                    key,
                    size: metadata.map(|metadata| metadata.len()),
                    modified,
                }
            })
            .collect();
    }
    pub(super) fn replace_providers(&mut self, kind: ProviderKind, mut providers: Vec<Provider>) {
        // A successful list refresh cannot prove that a timed-out update stopped.
        for previous in self
            .providers(kind)
            .iter()
            .filter(|provider| provider.uncertain)
        {
            if let Some(current) = providers
                .iter_mut()
                .find(|provider| provider.name == previous.name)
            {
                current.uncertain = true;
            }
        }
        match kind {
            ProviderKind::Proxies => self.proxies = providers,
            ProviderKind::Rules => self.rules = providers,
        }
    }
}
#[derive(Deserialize)]
struct Response {
    providers: BTreeMap<String, RawProvider>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawProvider {
    vehicle_type: String,
    updated_at: Option<String>,
    #[serde(default)]
    proxies: Vec<serde_json::Value>,
    #[serde(default)]
    rule_count: u64,
    behavior: Option<String>,
    subscription_info: Option<Usage>,
}
pub(super) fn parse(bytes: &[u8], kind: ProviderKind) -> Result<Vec<Provider>, String> {
    let response: Response = serde_json::from_slice(bytes).map_err(|_| "资源集合响应无效")?;
    if response.providers.len() > 4096 {
        return Err("资源集合数量超限".into());
    }
    response
        .providers
        .into_iter()
        .filter(|(_, provider)| !provider.vehicle_type.eq_ignore_ascii_case("compatible"))
        .map(|(name, raw)| {
            if name.is_empty() || name.len() > 1024 {
                return Err("资源集合名称无效".into());
            }
            Ok(Provider {
                name,
                vehicle: raw.vehicle_type,
                count: match kind {
                    ProviderKind::Proxies => raw.proxies.len() as u64,
                    ProviderKind::Rules => raw.rule_count,
                },
                updated: raw.updated_at.filter(|time| !time.starts_with("0001-")),
                behavior: raw.behavior,
                usage: raw.subscription_info,
                uncertain: false,
            })
        })
        .collect()
}
pub(super) fn escaped_name(name: &str) -> String {
    const PATH_COMPONENT: percent_encoding::AsciiSet = percent_encoding::NON_ALPHANUMERIC
        .remove(b'-')
        .remove(b'_')
        .remove(b'.')
        .remove(b'~');
    percent_encoding::utf8_percent_encode(name, &PATH_COMPONENT).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn providers_use_map_identity_filter_compatible_and_preserve_unknown_update_state() {
        let bytes = br#"{"providers":{"default":{"vehicleType":"Compatible"},"a/b?c":{"vehicleType":"HTTP","updatedAt":"2026-10-28T12:00:00Z","proxies":[{}]}}}"#;
        let parsed = parse(bytes, ProviderKind::Proxies).unwrap();
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].name(), "a/b?c");
        assert_eq!(parsed[0].count(), 1);
        assert_eq!(escaped_name(parsed[0].name()), "a%2Fb%3Fc");
        let mut snapshot = Resources {
            proxies: parsed.clone(),
            ..Resources::default()
        };
        snapshot.mark_uncertain(ProviderKind::Proxies, "a/b?c");
        snapshot.replace_providers(ProviderKind::Proxies, parsed);
        assert!(!snapshot.proxies[0].can_update());
    }
}
