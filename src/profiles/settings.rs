//! Persisted subscription metadata and selected identity share one settings section.
use super::{
    model::{Document, DocumentKind, TrafficUsage},
    store,
};
use crate::storage::{AppPaths, Values};
use serde::{Deserialize, Serialize};

#[derive(Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
#[non_exhaustive]
pub(crate) struct Settings {
    pub(crate) selected_profile_id: Option<u64>,
    items: Vec<Profile>,
}
#[derive(Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct Profile {
    id: u64,
    name: String,
    url: String,
    enabled: bool,
    update_interval_minutes: u64,
    request_timeout_seconds: u64,
    user_agent: String,
    with_proxy: bool,
    self_proxy: bool,
    accept_invalid_certs: bool,
    updated_at: Option<i64>,
    home: String,
    usage: Option<TrafficUsage>,
}
impl Default for Profile {
    fn default() -> Self {
        Self::from_document(Document::new(0, DocumentKind::Profile), 1440, 30)
    }
}
impl Profile {
    fn from_document(document: Document, interval: u64, timeout: u64) -> Self {
        Self {
            id: document.id,
            name: document.name,
            url: document.url,
            enabled: document.enabled,
            update_interval_minutes: interval,
            request_timeout_seconds: timeout,
            user_agent: document.user_agent,
            with_proxy: document.with_proxy,
            self_proxy: document.self_proxy,
            accept_invalid_certs: document.accept_invalid_certs,
            updated_at: document.updated_at,
            home: document.home,
            usage: document.usage,
        }
    }
    fn document(self) -> Document {
        let mut document = Document::new(self.id, DocumentKind::Profile);
        document.name = self.name;
        document.url = self.url;
        document.content.clear();
        document.enabled = self.enabled;
        document.interval = self.update_interval_minutes.to_string();
        document.timeout = self.request_timeout_seconds.to_string();
        document.user_agent = self.user_agent;
        document.with_proxy = self.with_proxy;
        document.self_proxy = self.self_proxy;
        document.accept_invalid_certs = self.accept_invalid_certs;
        document.updated_at = self.updated_at;
        document.home = self.home;
        document.usage = self.usage;
        document
    }
}
impl Settings {
    pub(crate) fn load(paths: &AppPaths) -> Result<Self, String> {
        Self::load_limited(paths, crate::configuration::INPUT_LIMIT)
    }
    fn load_limited(paths: &AppPaths, limit: u64) -> Result<Self, String> {
        let settings: Self = crate::preferences::app::read_limited(paths, "profiles", limit)?;
        let mut ids = std::collections::BTreeSet::new();
        if settings.items.iter().any(|profile| {
            profile.id == 0
                || !ids.insert(profile.id)
                || profile.update_interval_minutes > 525600
                || !(1..=3600).contains(&profile.request_timeout_seconds)
        }) {
            return Err(format!(
                "{}：订阅身份、更新间隔或请求超时无效",
                crate::preferences::app::path(paths).display()
            ));
        }
        Ok(settings)
    }
    pub(crate) fn save(&self, paths: &AppPaths) -> Result<(), String> {
        crate::preferences::app::write(paths, "profiles", self)
    }
}
pub(crate) fn read_values(paths: &AppPaths, limit: u64) -> Result<Values, String> {
    let settings = Settings::load_limited(paths, limit)?;
    let documents: Vec<_> = settings.items.into_iter().map(Profile::document).collect();
    Ok(store::metadata_values(&documents))
}
pub(crate) fn write_values(paths: &AppPaths, values: &Values) -> Result<(), String> {
    store::validate_metadata(paths, false, values)?;
    let documents = super::repository::decode(values)?;
    let mut settings = Settings::load(paths)?;
    let mut items = Vec::with_capacity(documents.len());
    for document in documents {
        let interval = document
            .interval
            .parse::<u64>()
            .map_err(|error| crate::logging::rejected("订阅更新间隔无效", error))?;
        let timeout = document
            .timeout
            .parse::<u64>()
            .map_err(|error| crate::logging::rejected("订阅请求超时无效", error))?;
        if interval > 525600 || !(1..=3600).contains(&timeout) {
            return Err("订阅更新间隔或请求超时无效".into());
        }
        items.push(Profile::from_document(document, interval, timeout));
    }
    settings.items = items;
    settings.save(paths)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn typed_profiles_and_selection_preserve_each_other_and_other_sections() {
        let root = tempfile::tempdir().unwrap();
        let paths = AppPaths::isolated(root.path());
        paths.create().unwrap();
        let mut document = Document::new(7, DocumentKind::Profile);
        document.name = "订阅 / agent".into();
        document.user_agent = "custom agent".into();
        document.interval = "0".into();
        write_values(&paths, &store::metadata_values(&[document])).unwrap();
        crate::configuration::select_profile(&paths, Some(7)).unwrap();
        let mut selections = crate::proxies::runtime::Selections::load(&paths).unwrap();
        selections.update(Some(7), "AI", Some("node"));
        selections.save(&paths).unwrap();
        let mut values = read_values(&paths, crate::configuration::INPUT_LIMIT).unwrap();
        values.insert("7/name".into(), "renamed".into());
        write_values(&paths, &values).unwrap();
        let json: serde_json::Value =
            serde_json::from_slice(&std::fs::read(crate::preferences::app::path(&paths)).unwrap())
                .unwrap();
        assert_eq!(json["profiles"]["selected_profile_id"], 7);
        let entry = &json["profiles"]["items"][0];
        assert_eq!(entry["id"], 7);
        assert_eq!(entry["name"], "renamed");
        assert_eq!(entry["update_interval_minutes"], 0);
        assert_eq!(entry["request_timeout_seconds"], 30);
        assert_eq!(entry["enabled"], true);
        assert_eq!(json["proxy_selections"][0]["profile_id"], 7);
        assert_eq!(json["proxy_selections"][0]["groups"]["AI"], "node");
        assert!(!json.to_string().contains("7/agent"));
        assert!(!paths.data().join("records").exists());
        assert_eq!(
            paths.traffic_record(),
            paths
                .directory(crate::storage::DirectoryKind::Cache)
                .join("traffic.json")
        );
        let before = crate::preferences::app::capture(&paths, &["profiles"]).unwrap();
        crate::configuration::select_profile(&paths, None).unwrap();
        crate::preferences::app::write(
            &paths,
            "general",
            &serde_json::json!({"theme_index":"dark"}),
        )
        .unwrap();
        selections.update(Some(7), "AI", Some("another node"));
        selections.save(&paths).unwrap();
        crate::preferences::app::restore(&paths, &before).unwrap();
        assert_eq!(
            crate::configuration::selected_profile(&paths).unwrap(),
            Some(7)
        );
        assert_eq!(
            read_values(&paths, crate::configuration::INPUT_LIMIT).unwrap()["7/name"],
            "renamed"
        );
        assert_eq!(
            crate::proxies::runtime::Selections::load(&paths)
                .unwrap()
                .for_profile(Some(7))["AI"],
            "another node"
        );
        let general: serde_json::Value = crate::preferences::app::read(&paths, "general").unwrap();
        assert_eq!(general["theme_index"], "dark");
    }
    #[test]
    fn invalid_profile_fields_do_not_replace_settings() {
        let root = tempfile::tempdir().unwrap();
        let paths = AppPaths::isolated(root.path());
        paths.create().unwrap();
        for items in [
            serde_json::json!([{"id":7,"enabled":"true"}]),
            serde_json::json!([{"id":7,"request_timeout_seconds":0}]),
            serde_json::json!([{"id":7},{"id":7}]),
        ] {
            crate::preferences::app::write(&paths, "profiles", &serde_json::json!({"items":items}))
                .unwrap();
            let before = std::fs::read(crate::preferences::app::path(&paths)).unwrap();
            assert!(read_values(&paths, crate::configuration::INPUT_LIMIT).is_err());
            assert!(crate::configuration::select_profile(&paths, Some(7)).is_err());
            assert_eq!(
                std::fs::read(crate::preferences::app::path(&paths)).unwrap(),
                before
            );
        }
    }
}
