//! Application settings contracts, defaults. No GPUI dependency.
pub(crate) mod app;
pub(crate) mod choices;
pub(crate) mod json;
mod kind;
pub(crate) mod native;
pub(crate) mod schema;
use crate::storage::{Namespace, Repository, StorageError, Values};
pub(crate) use kind::SettingsKind;

pub(crate) fn defaults(kind: SettingsKind) -> Values {
    let mut values: Values = schema::sections(kind)
        .into_iter()
        .flat_map(|section| section.fields)
        .filter(|field| field.kind != schema::FieldKind::ReadOnly)
        .map(|field| (field.key.to_owned(), field.default.to_owned()))
        .collect();
    if kind == SettingsKind::Core {
        schema::store_panels(&schema::default_panels(), &mut values);
        values.insert("StartCore".into(), "true".into());
    }
    if kind == SettingsKind::Resources {
        for (key, database) in [
            ("GeoIpUrl", crate::configuration::resources::Database::GeoIp),
            (
                "GeoSiteUrl",
                crate::configuration::resources::Database::GeoSite,
            ),
            (
                "MmdbUrl",
                crate::configuration::resources::Database::Country,
            ),
            ("AsnUrl", crate::configuration::resources::Database::Asn),
        ] {
            values.insert(key.into(), database.default_url().into());
        }
    }
    choices::encode(values)
}
pub(crate) fn kind_for_page(page: &str) -> Option<SettingsKind> {
    SettingsKind::ALL.into_iter().find(|kind| kind.id() == page)
}
pub(crate) fn values(repository: &Repository, kind: SettingsKind) -> Result<Values, StorageError> {
    let namespace = Namespace::parse(kind.id())?;
    let mut values = defaults(kind);
    values.extend(repository.read_limited(&namespace, crate::configuration::INPUT_LIMIT)?);
    Ok(values)
}
pub(crate) fn initialize(repository: &Repository) -> Result<(), StorageError> {
    crate::core::recover_pending(repository.paths()).map_err(std::io::Error::other)?;
    // Core and resource settings share one versioned native/app document, owned
    // by the core configuration workflow. User sources are never seeded here.
    let core = crate::configuration::Preferences::load(repository.paths())
        .map_err(std::io::Error::other)?;
    let mut pending = Vec::new();
    for kind in SettingsKind::ALL
        .into_iter()
        .filter(|kind| !matches!(kind, SettingsKind::Core | SettingsKind::Resources))
    {
        let namespace = Namespace::parse(kind.id())?;
        let merged = values(repository, kind)?;
        validate(kind, &merged).map_err(|error| {
            std::io::Error::other(format!(
                "{}：{error}；请修正文件后重试",
                repository.document_path(&namespace).display()
            ))
        })?;
        if repository.read(&namespace)?.is_empty() || repository.read(&namespace)? != merged {
            pending.push((namespace, merged));
        }
    }
    crate::configuration::initialize_selection(repository.paths())
        .map_err(std::io::Error::other)?;
    crate::proxies::chain_config::Config::load(repository.paths())
        .and_then(|chain| chain.save(repository.paths()))
        .map_err(std::io::Error::other)?;
    core.save(repository.paths())
        .map_err(std::io::Error::other)?;
    for (namespace, merged) in pending {
        repository.save(&namespace, &merged)?;
    }
    Ok(())
}
pub(crate) fn validate(kind: SettingsKind, values: &Values) -> Result<(), String> {
    let values = choices::decode(values.clone());
    if let Some(error) = schema::validate(&schema::sections(kind), &values) {
        return Err(error);
    }
    for field in schema::sections(kind)
        .into_iter()
        .flat_map(|section| section.fields)
    {
        if field.kind == schema::FieldKind::Toggle
            && values
                .get(field.key)
                .is_some_and(|value| !matches!(value.as_str(), "true" | "false"))
        {
            return Err(format!("{}必须为布尔值", field.label));
        }
        if field.kind == schema::FieldKind::Select
            && field.key != "CoreSpecificVersion"
            && values
                .get(field.key)
                .is_some_and(|value| !field.options.contains(&value.as_str()))
        {
            return Err(format!("{}无效", field.label));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::AppPaths;
    #[test]
    fn missing_settings_create_defaults_without_user_sources_and_reopen_keeps_edits() {
        let root = tempfile::tempdir().unwrap();
        let paths = AppPaths::isolated(root.path());
        let repository = Repository::open(paths.clone()).unwrap();
        initialize(&repository).unwrap();
        for name in ["settings", "dns", "sniffer", "mihomo"] {
            let extension = if matches!(name, "dns" | "sniffer" | "mihomo") {
                "yaml"
            } else {
                "json"
            };
            let path = paths.settings().join(format!("{name}.{extension}"));
            let value: serde_yaml_ng::Value =
                serde_yaml_ng::from_slice(&std::fs::read(path).unwrap()).unwrap();
            assert!(value.is_mapping());
        }
        assert_eq!(std::fs::read_dir(paths.settings()).unwrap().count(), 4);
        let program: serde_json::Value =
            serde_json::from_slice(&std::fs::read(crate::preferences::app::path(&paths)).unwrap())
                .unwrap();
        for section in [
            "general",
            "core",
            "dns",
            "sniffer",
            "sysproxy",
            "tun",
            "runtime",
            "proxy_chain",
        ] {
            assert!(program[section].is_object(), "{section}");
        }
        assert_eq!(program["dns"]["dns_override_enabled"], false);
        for name in ["dns", "sniffer"] {
            let yaml: serde_yaml_ng::Value = serde_yaml_ng::from_slice(
                &std::fs::read(paths.settings().join(format!("{name}.yaml"))).unwrap(),
            )
            .unwrap();
            assert!(yaml.get("override-enabled").is_none());
        }
        assert_eq!(
            values(&repository, SettingsKind::General).unwrap()["AppLogLevel"],
            "info"
        );
        assert_eq!(
            values(&repository, SettingsKind::General).unwrap()["NoticePosition"],
            "top_center"
        );
        assert!(!paths.settings().join("profile-index.json").exists());
        assert!(!paths.settings().join("override-index.json").exists());
        let core = crate::configuration::Preferences::load(&paths).unwrap();
        assert_eq!(core.values()["MixedPort"], "7897");
        assert!(!core.values()["GeoIpUrl"].is_empty());
        assert_eq!(core.values()["StartCore"], "true");
        assert_eq!(
            values(&repository, SettingsKind::General).unwrap()["ThemeIndex"],
            "dark"
        );
        let mut values = values(&repository, SettingsKind::General).unwrap();
        values.insert("UserAgent".into(), "原文 UserAgent".into());
        repository
            .save(&Namespace::parse("settings").unwrap(), &values)
            .unwrap();
        initialize(&repository).unwrap();
        assert_eq!(
            self::values(&repository, SettingsKind::General).unwrap()["UserAgent"],
            "原文 UserAgent"
        );
    }
    #[test]
    fn malformed_json_and_wrong_native_types_remain_unchanged() {
        let root = tempfile::tempdir().unwrap();
        let paths = AppPaths::isolated(root.path());
        let repository = Repository::open(paths.clone()).unwrap();
        initialize(&repository).unwrap();
        let native = paths.settings().join("mihomo.yaml");
        for bytes in [
            b"broken".as_slice(),
            br#"{"mixed-port":"wrong"}"#,
            br#"{"find-process-mode":"invalid"}"#,
            br#"{"profile":{"store-selected":"true"}}"#,
        ] {
            std::fs::write(&native, bytes).unwrap();
            assert!(initialize(&repository).is_err());
            assert_eq!(std::fs::read(&native).unwrap(), bytes);
        }
    }
}

#[cfg(test)]
mod current_format_tests {
    use super::*;
    use crate::storage::{AppPaths, DirectoryKind};
    #[test]
    fn startup_initialization_uses_only_current_settings_and_leaves_other_files_alone() {
        let root = tempfile::tempdir().unwrap();
        let paths = AppPaths::isolated(root.path());
        let repository = Repository::open(paths.clone()).unwrap();
        let config = paths.directory(DirectoryKind::Config);
        let yaml = config.join("config.yaml");
        let snapshot = config.join("applied.json");
        std::fs::write(&yaml, "mixed-port: 18888\n").unwrap();
        std::fs::write(&snapshot, b"unread format").unwrap();
        initialize(&repository).unwrap();
        assert_eq!(
            crate::configuration::Preferences::load(&paths)
                .unwrap()
                .values()["MixedPort"],
            "7897"
        );
        assert_eq!(
            std::fs::read_to_string(&yaml).unwrap(),
            "mixed-port: 18888\n"
        );
        assert_eq!(std::fs::read(&snapshot).unwrap(), b"unread format");
        let selection: serde_json::Value =
            serde_json::from_slice(&std::fs::read(paths.settings().join("settings.json")).unwrap())
                .unwrap();
        assert_eq!(
            selection["runtime"]["selected_profile_id"],
            serde_json::Value::Null
        );
        assert!(
            !paths
                .directory(DirectoryKind::Backups)
                .join("settings-migration-v1")
                .exists()
        );
    }
}
