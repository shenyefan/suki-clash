use super::*;

fn index(items: &[(u64, &str, &str, bool, Option<u64>)]) -> Values {
    let mut values = Values::from([
        ("format".into(), "1".into()),
        (
            "ids".into(),
            items
                .iter()
                .map(|item| item.0.to_string())
                .collect::<Vec<_>>()
                .join(","),
        ),
    ]);
    for (id, kind, content, enabled, profile) in items {
        for (field, value) in [
            ("kind", kind.to_string()),
            ("content", content.to_string()),
            ("enabled", enabled.to_string()),
            (
                "scope-profile",
                profile.map(|id| id.to_string()).unwrap_or_default(),
            ),
        ] {
            values.insert(format!("{id}/{field}"), value);
        }
    }
    values
}
fn result(source: &str, profile: Option<u64>, values: &Values) -> Value {
    serde_yaml_ng::from_slice(&compose(source.as_bytes(), profile, values).unwrap()).unwrap()
}

#[test]
fn saved_order_interleaves_scopes_without_sorting_ids() {
    let values = index(&[
        (
            90,
            "YAML",
            "nested: {winner: first, retained: true}",
            true,
            None,
        ),
        (2, "YAML", "nested: {winner: second}", true, Some(7)),
        (1, "YAML", "nested: {winner: last}", true, None),
    ]);
    let output = result("nested: {original: true}", Some(7), &values);
    assert_eq!(output["nested"]["winner"], "last");
    assert_eq!(output["nested"]["retained"], true);
    assert_eq!(output["nested"]["original"], true);
}

#[test]
fn cv_arrays_and_null_replace_dns_subobjects_replace_and_empty_ordinary_maps_preserve() {
    let values = index(&[(
        1,
        "YAML",
        "dns: {nameserver: [], retained: {}, removed: null}\nrules: [MATCH, DIRECT]\nreplaced: {absent: null, valid: 1}",
        true,
        None,
    )]);
    let output = result(
        "dns: {nameserver: [old], retained: {key: value}, removed: true}\nrules: [old]\nreplaced: scalar",
        None,
        &values,
    );
    assert_eq!(output["dns"]["nameserver"], Value::Sequence(vec![]));
    assert_eq!(output["dns"]["retained"], Value::Mapping(Mapping::new()));
    assert!(output["dns"].get("removed").unwrap().is_null());
    assert_eq!(
        output["rules"],
        serde_yaml_ng::from_str::<Value>("[MATCH, DIRECT]").unwrap()
    );
    assert!(output["replaced"].get("absent").unwrap().is_null());
    assert_eq!(output["replaced"]["valid"], 1);
}

#[test]
fn names_unknown_fields_and_original_nulls_are_preserved() {
    let values = index(&[(
        1,
        "YAML",
        "custom: {MixedCase: changed, mixedcase: distinct}",
        true,
        None,
    )]);
    let output = result(
        "custom: {MixedCase: old, untouched: null}\nproxy-groups: [{name: MyGroup, proxies: [MyNode]}]\nrules: ['DOMAIN,ExAmPlE.com,MyGroup']",
        None,
        &values,
    );
    assert_eq!(output["custom"]["MixedCase"], "changed");
    assert_eq!(output["custom"]["mixedcase"], "distinct");
    assert!(
        output["custom"]
            .as_mapping()
            .unwrap()
            .contains_key("untouched")
    );
    assert_eq!(output["proxy-groups"][0]["name"], "MyGroup");
    assert_eq!(output["rules"][0], "DOMAIN,ExAmPlE.com,MyGroup");
}

#[test]
fn disabled_and_unmatched_scripts_are_never_executed_or_parsed() {
    let values = index(&[
        (1, "JavaScript", "throw new Error('disabled')", false, None),
        (
            2,
            "JavaScript",
            "throw new Error('other profile')",
            true,
            Some(8),
        ),
        (3, "YAML", "[invalid", true, Some(7)),
    ]);
    assert_eq!(result("mode: rule", None, &values)["mode"], "rule");
    assert!(compose(b"mode: rule", Some(7), &values).is_err());
}

#[test]
fn applicable_script_fails_at_its_position_in_the_saved_chain() {
    let values = index(&[
        (9, "YAML", "mode: global", true, None),
        (3, "JavaScript", "function main(c) { throw 1; }", true, None),
        (1, "YAML", "[invalid", true, None),
    ]);
    assert!(matches!(
        compose(b"mode: rule", None, &values),
        Err(CompositionError::Script(3, _))
    ));
}

#[test]
fn malformed_indexes_never_silently_drop_overrides() {
    let good = index(&[(1, "YAML", "{}", true, None)]);
    for (key, value) in [
        ("format", "2"),
        ("ids", "1,1"),
        ("ids", "1,bad"),
        ("1/enabled", "yes"),
        ("1/kind", "unknown"),
        ("1/scope-profile", "not-an-id"),
    ] {
        let mut values = good.clone();
        values.insert(key.into(), value.into());
        assert!(compose(b"{}", None, &values).is_err(), "{key}");
    }
    let mut values = good;
    values.remove("1/content");
    assert!(compose(b"{}", None, &values).is_err());
}

#[test]
fn source_and_patch_must_be_objects_and_errors_do_not_leak_credentials() {
    for source in ["[]", "null", "", "secret: [super-private-credential"] {
        let error = compose(source.as_bytes(), None, &Values::new())
            .unwrap_err()
            .to_string();
        assert!(!error.contains("super-private-credential"));
    }
    for content in ["[]", "null", "", "secret: [super-private-credential"] {
        let values = index(&[(42, "YAML", content, true, None)]);
        let error = compose(b"{}", None, &values).unwrap_err().to_string();
        assert!(error.contains("42"));
        assert!(!error.contains("super-private-credential"));
    }
}

#[test]
fn bounded_input_depth_and_total_override_count() {
    let oversized = vec![b' '; INPUT_LIMIT as usize + 1];
    assert!(matches!(
        compose(&oversized, None, &Values::new()),
        Err(CompositionError::Budget(_))
    ));
    let deep = format!(
        "root: {}0{}",
        "[".repeat(DEPTH_LIMIT + 1),
        "]".repeat(DEPTH_LIMIT + 1)
    );
    assert!(compose(deep.as_bytes(), None, &Values::new()).is_err());
    let items = (0..=OVERRIDE_LIMIT as u64)
        .map(|id| (id, "YAML", "{}", true, None))
        .collect::<Vec<_>>();
    assert!(matches!(
        compose(b"{}", None, &index(&items)),
        Err(CompositionError::InvalidIndex)
    ));
}

#[test]
fn absent_or_empty_indexes_leave_source_intact() {
    for values in [Values::new(), index(&[])] {
        assert_eq!(result("mode: rule", None, &values)["mode"], "rule");
    }
}

#[tokio::test]
async fn selected_profile_reloads_current_source_and_bad_selection_never_falls_back() {
    use crate::storage::{AppPaths, DirectoryKind, Namespace, Repository};
    let root = tempfile::tempdir().unwrap();
    let paths = AppPaths::isolated(root.path());
    let store = Repository::open(paths.clone()).unwrap();
    let namespace = Namespace::parse("profile-index").unwrap();
    let mut values = Values::from([
        ("format".into(), "1".into()),
        ("ids".into(), "7".into()),
        ("7/kind".into(), "配置".into()),
        ("7/name".into(), "真实本地配置".into()),
        ("7/content".into(), "mode: rule".into()),
    ]);
    store.save(&namespace, &values).unwrap();
    let source = Source::from_profile(&paths, 7).unwrap();
    assert_eq!(source.profile().unwrap().id(), 7);
    assert_eq!(source.profile().unwrap().name(), "真实本地配置");
    source.commit(&paths).unwrap();
    values.insert("7/content".into(), "mode: direct".into());
    store.save(&namespace, &values).unwrap();
    let loaded = Source::load(&paths).await.unwrap();
    assert_eq!(
        loaded.profile().unwrap().id(),
        source.profile().unwrap().id()
    );
    assert_eq!(loaded.content(), b"mode: direct");
    let selection = paths.settings().join("settings.json");
    let bytes = std::fs::read(&selection).unwrap();
    let record: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(record["runtime"]["selected_profile_id"], 7);
    assert!(record.get("content").is_none());
    std::fs::write(
        &selection,
        br#"{"schema_version":1,"runtime":{"selected_profile_id":99}}"#,
    )
    .unwrap();
    std::fs::write(
        paths
            .directory(DirectoryKind::Config)
            .join("runtime-source.yaml"),
        "mode: direct",
    )
    .unwrap();
    assert!(Source::load(&paths).await.is_err());
}

#[test]
fn cv_lowercases_root_keys_but_preserves_nested_names_and_replaces_hosts() {
    let values = index(&[(
        1,
        "YAML",
        "DNS: {nameserver-policy: {New.Example: 1.1.1.1}}\nHosts: {}\nCustomRoot: {InnerName: new}\nordinary: {}",
        true,
        None,
    )]);
    let output = result(
        "DNS: {nameserver: [8.8.8.8], nameserver-policy: {Old.Example: 9.9.9.9}}\nHosts: {LocalHost: 127.0.0.1}\nCustomRoot: {RetainedName: old}\nordinary: {retained: true}",
        None,
        &values,
    );
    assert!(output.get("DNS").is_none());
    assert_eq!(output["dns"]["nameserver"][0], "8.8.8.8");
    assert!(
        output["dns"]["nameserver-policy"]
            .get("Old.Example")
            .is_none()
    );
    assert_eq!(output["dns"]["nameserver-policy"]["New.Example"], "1.1.1.1");
    assert_eq!(output["hosts"], Value::Mapping(Mapping::new()));
    assert_eq!(output["customroot"]["RetainedName"], "old");
    assert_eq!(output["customroot"]["InnerName"], "new");
    assert_eq!(output["ordinary"]["retained"], true);
}

#[tokio::test]
async fn empty_profile_list_produces_a_real_basic_runtime_without_fake_profile_identity() {
    use crate::storage::{AppPaths, DirectoryKind, Repository};
    let root = tempfile::tempdir().unwrap();
    let paths = AppPaths::isolated(root.path());
    Repository::open(paths.clone()).unwrap();
    let source = Source::load(&paths).await.unwrap();
    assert!(source.profile().is_none());
    let value: Value = serde_yaml_ng::from_slice(source.content()).unwrap();
    assert_eq!(value["mixed-port"], 7897);
    assert_eq!(value["mode"], "rule");
    assert!(value.get("proxies").is_none());
    assert!(value.get("rules").is_none());
    assert!(
        !paths
            .directory(DirectoryKind::Config)
            .join("runtime-source.yaml")
            .exists()
    );
}
