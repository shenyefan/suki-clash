use super::*;

fn repository(root: &Path) -> Repository {
    Repository::open(AppPaths::isolated(root), None).unwrap()
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
fn migration_preserves_sources_and_never_replaces_newer_settings() {
    let root = tempfile::tempdir().unwrap();
    let preview = root.path().join("preview");
    fs::create_dir_all(preview.join("profiles")).unwrap();
    fs::write(
        preview.join("settings.settings"),
        "ThemeIndex\t浅色\ntext\t%250A%0A中文\n",
    )
    .unwrap();
    fs::write(preview.join("logs.settings"), "source\t1\n").unwrap();
    fs::write(preview.join("profiles/7.yaml"), "proxies: []\n").unwrap();
    let paths = AppPaths::isolated(&root.path().join("official"));
    let store = Repository::open(paths.clone(), None).unwrap();
    let newer = Values::from([("ThemeIndex".into(), "深色".into())]);
    store.save(&namespace(), &newer).unwrap();
    let migrated = Repository::open(paths.clone(), Some(&preview)).unwrap();
    assert_eq!(migrated.read(&namespace()).unwrap(), newer);
    assert_eq!(
        migrated.read(&Namespace::parse("logs").unwrap()).unwrap()["source"],
        "1"
    );
    assert_eq!(
        fs::read_to_string(paths.data().join("profiles/7.yaml")).unwrap(),
        "proxies: []\n"
    );
    assert!(preview.join("settings.settings").is_file());
    // A later boot must not resurrect an old file that was intentionally removed.
    fs::remove_file(paths.settings().join("logs.json")).unwrap();
    Repository::open(paths.clone(), Some(&preview)).unwrap();
    assert!(!paths.settings().join("logs.json").exists());
}

#[test]
fn legacy_escape_decoding_does_not_decode_literal_percent_sequences_twice() {
    let values = parse_legacy("text\t%250A%0A中文%09%0D%2525\n").unwrap();
    assert_eq!(values["text"], "%0A\n中文\t\r%25");
    assert!(parse_legacy("missing separator").is_err());
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
    fs::write(&path, r#"{"version":99,"values":{}}"#).unwrap();
    assert!(matches!(
        store.snapshot(),
        Err(StorageError::UnsupportedVersion(99))
    ));
    fs::write(&path, b"broken").unwrap();
    assert!(matches!(
        store.snapshot(),
        Err(StorageError::InvalidDocument(_))
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

#[cfg(unix)]
#[test]
fn migration_rejects_symlinks_and_does_not_mark_an_incomplete_migration_complete() {
    let root = tempfile::tempdir().unwrap();
    let preview = root.path().join("preview");
    fs::create_dir(&preview).unwrap();
    let outside = root.path().join("outside");
    fs::write(&outside, "theme\tsecret\n").unwrap();
    std::os::unix::fs::symlink(&outside, preview.join("settings.settings")).unwrap();
    let paths = AppPaths::isolated(&root.path().join("official"));
    assert!(matches!(
        Repository::open(paths.clone(), Some(&preview)),
        Err(StorageError::UnsafeMigration(_))
    ));
    assert!(!paths.settings().join(".preview-migrated-v1").exists());
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
