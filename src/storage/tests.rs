use super::*;

fn repository(root: &Path) -> Repository {
    Repository::open(AppPaths::isolated(root)).unwrap()
}
fn namespace() -> Namespace {
    Namespace::parse("settings").unwrap()
}

#[test]
fn replace_existing_settings_and_roundtrip_unicode_and_escapes() {
    let root = tempfile::tempdir().unwrap();
    let store = repository(root.path());
    store
        .save(&namespace(), &Values::from([("old".into(), "old".into())]))
        .unwrap();
    let values = Values::from([("dns".into(), "localhost\n中文\t%0A\r%25".into())]);
    store.save(&namespace(), &values).unwrap();
    assert_eq!(repository(root.path()).read(&namespace()).unwrap(), values);
    assert_eq!(fs::read_dir(store.paths.settings()).unwrap().count(), 1);
}

#[test]
fn bounded_reads_reject_oversized_storage_without_changing_it() {
    let root = tempfile::tempdir().unwrap();
    let store = repository(root.path());
    assert!(store.read_limited(&namespace(), 0).unwrap().is_empty());
    let values = Values::from([("content".into(), "原始配置".into())]);
    store.save(&namespace(), &values).unwrap();
    let before = fs::read(store.path(&namespace())).unwrap();
    assert!(
        store
            .read_limited(&namespace(), before.len() as u64 - 1)
            .is_err()
    );
    assert_eq!(
        store
            .read_limited(&namespace(), before.len() as u64)
            .unwrap(),
        values
    );
    assert_eq!(fs::read(store.path(&namespace())).unwrap(), before);
}

#[test]
fn invalid_names_cannot_escape_storage_directory() {
    for name in [
        "../settings",
        "a/b",
        "C:\\settings",
        "settings.json",
        "profile-enhance-../x",
        "profile-enhance-",
    ] {
        assert!(Namespace::parse(name).is_err(), "{name}");
    }
    assert!(Namespace::parse("profile-enhance-42").is_ok());
}

#[test]
fn unsupported_or_corrupt_storage_is_not_silently_replaced() {
    let root = tempfile::tempdir().unwrap();
    let store = repository(root.path());
    let path = store.path(&namespace());
    fs::write(&path, r#"{"schema_version":99}"#).unwrap();
    assert!(matches!(
        store.snapshot(),
        Err(StorageError::Document { .. } | StorageError::Settings(_))
    ));
    fs::write(&path, b"broken").unwrap();
    assert!(matches!(
        store.snapshot(),
        Err(StorageError::Document { .. } | StorageError::Settings(_))
    ));
    assert_eq!(fs::read(&path).unwrap(), b"broken");
}

#[test]
fn restoring_backup_returns_a_draft_without_overwriting_committed_settings() {
    let root = tempfile::tempdir().unwrap();
    let store = repository(root.path());
    let before = Values::from([("theme".into(), "light".into())]);
    let after = Values::from([("theme".into(), "dark".into())]);
    store.save(&namespace(), &before).unwrap();
    let backup = store.backup(&namespace()).unwrap();
    store.save(&namespace(), &after).unwrap();
    assert_eq!(store.restore(&namespace()).unwrap(), before);
    assert_eq!(store.read(&namespace()).unwrap(), after);
    fs::write(backup, b"broken backup").unwrap();
    assert!(store.restore(&namespace()).is_err());
    assert_eq!(store.read(&namespace()).unwrap(), after);
}

#[test]
fn credential_fields_are_not_written_into_settings_or_backups() {
    let root = tempfile::tempdir().unwrap();
    let store = repository(root.path());
    let values = Values::from([
        ("ThemeIndex".into(), "深色".into()),
        ("GitHubToken".into(), "private-token".into()),
        ("WebDavPassword".into(), "private-password".into()),
        ("Secret".into(), "controller-secret".into()),
        ("GistAgeSecretKey".into(), "private-key".into()),
    ]);
    assert!(contains_session_credentials(&values));
    store.save(&namespace(), &values).unwrap();
    let backup = store.backup(&namespace()).unwrap();
    for path in [store.path(&namespace()), backup] {
        let persisted = fs::read_to_string(path).unwrap();
        assert!(!persisted.contains("private-"));
        assert!(!persisted.contains("controller-secret"));
    }
    assert_eq!(
        store.read(&namespace()).unwrap(),
        Values::from([("ThemeIndex".into(), "深色".into())])
    );
}

#[test]
fn snapshot_reads_only_registered_documents_and_preserves_other_files() {
    let root = tempfile::tempdir().unwrap();
    let store = repository(root.path());
    let unrelated = store.paths().settings().join("logs.json");
    fs::write(&unrelated, b"unowned document").unwrap();
    let values = Values::from([("ThemeIndex".into(), "dark".into())]);
    store.save(&namespace(), &values).unwrap();
    assert_eq!(store.snapshot().unwrap()[&namespace()], values);
    assert_eq!(fs::read(unrelated).unwrap(), b"unowned document");
}

#[test]
fn native_backup_keeps_application_switch_separate_and_restore_does_not_write_settings() {
    let root = tempfile::tempdir().unwrap();
    let store = repository(root.path());
    let dns = Namespace::parse("dns").unwrap();
    let mut values = crate::preferences::defaults(crate::preferences::SettingsKind::Dns);
    values.insert("DnsOverrideEnabled".into(), "true".into());
    store.save(&dns, &values).unwrap();
    let backup = store.backup(&dns).unwrap();
    let document: serde_json::Value = serde_json::from_slice(&fs::read(backup).unwrap()).unwrap();
    assert_eq!(document["enabled"], true);
    assert!(document["configuration"].get("override-enabled").is_none());
    values.insert("DnsOverrideEnabled".into(), "false".into());
    store.save(&dns, &values).unwrap();
    let before = fs::read(crate::preferences::app::path(store.paths())).unwrap();
    assert_eq!(store.restore(&dns).unwrap()["DnsOverrideEnabled"], "true");
    assert_eq!(
        fs::read(crate::preferences::app::path(store.paths())).unwrap(),
        before
    );
    assert_eq!(store.read(&dns).unwrap()["DnsOverrideEnabled"], "false");
}
