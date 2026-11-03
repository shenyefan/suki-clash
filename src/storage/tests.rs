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
fn credential_fields_are_not_written_into_settings() {
    let root = tempfile::tempdir().unwrap();
    let store = repository(root.path());
    let values = Values::from([
        ("ThemeIndex".into(), "深色".into()),
        ("GitHubToken".into(), "private-token".into()),
        ("Secret".into(), "controller-secret".into()),
        ("GistAgeSecretKey".into(), "private-key".into()),
    ]);
    assert!(contains_session_credentials(&values));
    store.save(&namespace(), &values).unwrap();
    let persisted = fs::read_to_string(store.path(&namespace())).unwrap();
    assert!(!persisted.contains("private-"));
    assert!(!persisted.contains("controller-secret"));
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
