use super::*;
use crate::storage::Repository;
use std::cell::Cell;

fn fixture() -> (tempfile::TempDir, AppPaths, PathBuf) {
    let root = tempfile::tempdir().unwrap();
    let paths = AppPaths::isolated(&root.path().join("user"));
    Repository::open(paths.clone()).unwrap();
    let bundle = root.path().join("bundle");
    fs::create_dir(&bundle).unwrap();
    let assets = [core_name(), GEO[0], GEO[1], GEO[2], GEO[3]]
        .into_iter()
        .map(|name| {
            let bytes = format!("fixture bytes for {name}").into_bytes();
            fs::write(bundle.join(name), &bytes).unwrap();
            Asset {
                name: name.into(),
                sha256: Sha256::digest(&bytes)
                    .iter()
                    .map(|byte| format!("{byte:02x}"))
                    .collect::<String>(),
                size: bytes.len() as u64,
            }
        })
        .collect();
    write_manifest(
        &bundle,
        &Manifest {
            version: 1,
            target: target(),
            assets,
        },
    );
    (root, paths, bundle)
}
fn write_manifest(bundle: &Path, manifest: &Manifest) {
    fs::write(
        bundle.join("manifest.json"),
        serde_json::to_vec(manifest).unwrap(),
    )
    .unwrap();
}

#[test]
fn first_open_installs_all_files_and_second_open_keeps_user_versions() {
    let (_root, paths, bundle) = fixture();
    prepare(&paths, &bundle, || false).unwrap();
    let directory = paths.directory(DirectoryKind::Core);
    for name in [core_name(), GEO[0], GEO[1], GEO[2], GEO[3]] {
        assert_eq!(
            fs::read(directory.join(name)).unwrap(),
            fs::read(bundle.join(name)).unwrap()
        );
    }
    fs::write(directory.join("Country.mmdb"), b"user update").unwrap();
    fs::remove_dir_all(&bundle).unwrap();
    prepare(&paths, &bundle, || false).unwrap();
    assert_eq!(
        fs::read(directory.join("Country.mmdb")).unwrap(),
        b"user update"
    );
    assert!(
        !paths
            .directory(DirectoryKind::Config)
            .join("runtime-source.yaml")
            .exists()
    );
}

#[test]
fn failed_integrity_never_installs_partial_bytes_and_retry_heals_missing_files() {
    let (_root, paths, bundle) = fixture();
    let original = fs::read(bundle.join("Country.mmdb")).unwrap();
    let mut corrupt = original.clone();
    corrupt[0] ^= 1;
    fs::write(bundle.join("Country.mmdb"), &corrupt).unwrap();
    assert!(
        prepare(&paths, &bundle, || false)
            .unwrap_err()
            .contains("SHA-256")
    );
    let directory = paths.directory(DirectoryKind::Core);
    assert!(directory.join(core_name()).is_file());
    assert!(!directory.join("Country.mmdb").exists());
    assert_eq!(fs::read_dir(&directory).unwrap().count(), 1);
    fs::write(bundle.join("Country.mmdb"), original).unwrap();
    prepare(&paths, &bundle, || false).unwrap();
    assert_eq!(fs::read_dir(directory).unwrap().count(), 5);
}

#[test]
fn wrong_platform_duplicate_and_traversal_names_fail_before_installing() {
    for mode in 0..3 {
        let (_root, paths, bundle) = fixture();
        let mut manifest: Manifest =
            serde_json::from_slice(&fs::read(bundle.join("manifest.json")).unwrap()).unwrap();
        match mode {
            0 => manifest.target = "wrong-target".into(),
            1 => manifest.assets[1].name = core_name().into(),
            _ => manifest.assets[1].name = "../outside".into(),
        }
        write_manifest(&bundle, &manifest);
        assert!(prepare(&paths, &bundle, || false).is_err());
        assert_eq!(
            fs::read_dir(paths.directory(DirectoryKind::Core))
                .unwrap()
                .count(),
            0
        );
    }
}

#[test]
fn missing_bundle_is_allowed_but_empty_installed_file_is_rejected() {
    let (_root, paths, bundle) = fixture();
    fs::remove_file(bundle.join("manifest.json")).unwrap();
    prepare(&paths, &bundle, || false).unwrap();
    assert!(require_core(&paths).unwrap_err().contains("「内核」"));
    assert!(geodata_notice(&paths).unwrap().contains("自动下载"));
    fs::write(paths.directory(DirectoryKind::Core).join(core_name()), []).unwrap();
    assert!(
        prepare(&paths, &bundle, || false)
            .unwrap_err()
            .contains("文件为空")
    );
}

#[test]
fn cancellation_during_copy_leaves_no_partial_destination() {
    let (_root, paths, bundle) = fixture();
    let calls = Cell::new(0);
    let result = prepare(&paths, &bundle, || {
        calls.set(calls.get() + 1);
        calls.get() > 6
    });
    assert!(result.unwrap_err().contains("取消"));
    assert_eq!(
        fs::read_dir(paths.directory(DirectoryKind::Core))
            .unwrap()
            .count(),
        0
    );
}

#[cfg(unix)]
#[test]
fn symlink_destination_is_rejected_without_touching_its_target() {
    let (root, paths, bundle) = fixture();
    let outside = root.path().join("outside");
    fs::write(&outside, b"do not change").unwrap();
    std::os::unix::fs::symlink(
        &outside,
        paths.directory(DirectoryKind::Core).join(core_name()),
    )
    .unwrap();
    assert!(prepare(&paths, &bundle, || false).is_err());
    assert_eq!(fs::read(outside).unwrap(), b"do not change");
}

#[cfg(unix)]
#[test]
fn executable_permissions_are_set_before_publish() {
    use std::os::unix::fs::PermissionsExt as _;
    let (_root, paths, bundle) = fixture();
    prepare(&paths, &bundle, || false).unwrap();
    let directory = paths.directory(DirectoryKind::Core);
    assert_eq!(
        fs::metadata(directory.join(core_name()))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o700
    );
    assert_eq!(
        fs::metadata(directory.join("Country.mmdb"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
}

#[test]
fn installed_core_does_not_require_bundled_geodata() {
    let (_root, paths, bundle) = fixture();
    let directory = paths.directory(DirectoryKind::Core);
    fs::write(directory.join(core_name()), b"user core").unwrap();
    fs::remove_dir_all(&bundle).unwrap();
    prepare(&paths, &bundle, || false).unwrap();
    require_core(&paths).unwrap();
    let notice = geodata_notice(&paths).unwrap();
    for name in GEO {
        assert!(notice.contains(name));
        fs::write(directory.join(name), b"downloaded by core").unwrap();
    }
    assert!(geodata_notice(&paths).is_none());
    assert_eq!(fs::read(directory.join(core_name())).unwrap(), b"user core");
}

#[test]
fn partial_bundle_installs_available_assets_and_allows_core_download() {
    let (_root, paths, bundle) = fixture();
    fs::remove_file(bundle.join(core_name())).unwrap();
    fs::remove_file(bundle.join("geoip.dat")).unwrap();
    prepare(&paths, &bundle, || false).unwrap();
    assert!(require_core(&paths).unwrap_err().contains("下载内核"));
    assert!(
        paths
            .directory(DirectoryKind::Core)
            .join("Country.mmdb")
            .is_file()
    );
    let notice = geodata_notice(&paths).unwrap();
    assert!(notice.contains("geoip.dat"));
    assert!(!notice.contains("Country.mmdb"));
}
