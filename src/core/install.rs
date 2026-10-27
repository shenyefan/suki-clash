//! Recoverable core replacement journal. All destinations are fixed application files.
use crate::storage::{AppPaths, DirectoryKind, atomic_write};
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};
use std::{
    fs,
    io::Read as _,
    path::{Path, PathBuf},
};
#[derive(Serialize, Deserialize)]
struct Manifest {
    version: u32,
    committed: bool,
    files: Vec<Entry>,
}
#[derive(Serialize, Deserialize)]
struct Entry {
    role: String,
    present: bool,
    size: u64,
    sha256: String,
}
pub(super) struct Recovery {
    root: PathBuf,
    paths: AppPaths,
    manifest: Manifest,
}
fn targets(paths: &AppPaths) -> Vec<(&'static str, PathBuf)> {
    let [native, app] = super::Preferences::paths(paths);
    vec![
        (
            "binary",
            paths
                .directory(DirectoryKind::Core)
                .join(crate::bootstrap::core_name()),
        ),
        ("native", native),
        ("app", app),
    ]
}
fn read(path: &Path) -> Result<Option<Vec<u8>>, String> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err("无法检查更新恢复材料".into()),
    };
    if !metadata.is_file()
        || metadata.file_type().is_symlink()
        || metadata.len() > super::download::ARCHIVE_LIMIT
    {
        return Err("更新恢复材料不是受支持的普通文件".into());
    }
    let mut bytes = Vec::new();
    fs::File::open(path)
        .map_err(|_| "无法读取更新恢复材料")?
        .take(super::download::ARCHIVE_LIMIT + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "读取恢复材料失败")?;
    if bytes.len() as u64 > super::download::ARCHIVE_LIMIT {
        return Err("恢复材料超过上限".into());
    }
    Ok(Some(bytes))
}
impl Recovery {
    pub(super) fn begin(paths: &AppPaths) -> Result<Self, String> {
        let root = paths
            .directory(DirectoryKind::Core)
            .join(".core-update-recovery");
        if root.exists() {
            return Err("已有未完成的内核更新，请重启应用完成恢复".into());
        }
        let staging = tempfile::Builder::new()
            .prefix(".recovery-stage-")
            .tempdir_in(paths.directory(DirectoryKind::Core))
            .map_err(|_| "无法创建更新恢复目录")?;
        let staged_root = staging.path().to_owned();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            fs::set_permissions(&staged_root, fs::Permissions::from_mode(0o700))
                .map_err(|_| "无法保护恢复目录")?;
        }
        let mut manifest = Manifest {
            version: 2,
            committed: false,
            files: Vec::new(),
        };
        for (role, path) in targets(paths) {
            let content = if role == "app" {
                Some(
                    serde_json::to_vec(&crate::preferences::app::capture(
                        paths,
                        &["core", "runtime"],
                    )?)
                    .map_err(|_| "无法保存内核设置恢复材料")?,
                )
            } else {
                read(&path)?
            };
            if let Some(bytes) = &content {
                atomic_write(&staged_root.join(role), bytes)
                    .map_err(|_| "无法保存旧内核恢复材料")?;
            }
            manifest.files.push(Entry {
                role: role.into(),
                present: content.is_some(),
                size: content.as_ref().map_or(0, |b| b.len() as u64),
                sha256: content.map_or_else(String::new, |b| {
                    Sha256::digest(b)
                        .iter()
                        .map(|byte| format!("{byte:02x}"))
                        .collect::<String>()
                }),
            });
        }
        let mut result = Self {
            root: staged_root,
            paths: paths.clone(),
            manifest,
        };
        result.write_manifest()?;
        fs::rename(&result.root, &root).map_err(|_| "无法提交内核更新恢复目录")?;
        result.root = root;
        Ok(result)
    }
    fn write_manifest(&self) -> Result<(), String> {
        atomic_write(
            &self.root.join("manifest.json"),
            &serde_json::to_vec(&self.manifest).map_err(|_| "无法编码恢复记录")?,
        )
        .map_err(|_| "无法提交恢复记录".into())
    }
    pub(super) fn commit(&mut self) -> Result<(), String> {
        self.manifest.committed = true;
        self.write_manifest()?;
        if let Err(error) = fs::remove_dir_all(&self.root) {
            eprintln!("内核更新已提交，但恢复目录清理失败：{}", error.kind());
        }
        Ok(())
    }
    pub(super) fn restore(self) -> Result<(), String> {
        if self.manifest.version != 2 || self.manifest.files.len() != 3 {
            return Err("内核恢复记录版本或文件数量无效".into());
        }
        // Verify every backup before changing any destination.
        let mut verified = Vec::new();
        for (role, path) in targets(&self.paths) {
            let entries = self
                .manifest
                .files
                .iter()
                .filter(|entry| entry.role == role)
                .collect::<Vec<_>>();
            if entries.len() != 1 {
                return Err("内核恢复记录缺失或重复".into());
            }
            let entry = entries[0];
            let bytes = if entry.present {
                let bytes = read(&self.root.join(role))?.ok_or("内核恢复文件缺失")?;
                if bytes.len() as u64 != entry.size
                    || Sha256::digest(&bytes)
                        .iter()
                        .map(|byte| format!("{byte:02x}"))
                        .collect::<String>()
                        != entry.sha256
                {
                    return Err("内核恢复文件校验失败".into());
                }
                Some(bytes)
            } else {
                None
            };
            verified.push((role, path, bytes));
        }
        for (role, path, bytes) in verified {
            match bytes {
                Some(bytes) if role == "app" => {
                    let snapshot =
                        serde_json::from_slice(&bytes).map_err(|_| "内核设置恢复材料无效")?;
                    crate::preferences::app::restore(&self.paths, &snapshot)?;
                }
                Some(bytes) => atomic_write(&path, &bytes).map_err(|_| "旧内核文件恢复失败")?,
                None => {
                    if path.exists() {
                        fs::remove_file(&path).map_err(|_| "无法撤销新增内核文件")?;
                    }
                }
            }
            #[cfg(unix)]
            if role == "binary" && path.exists() {
                use std::os::unix::fs::PermissionsExt as _;
                fs::set_permissions(&path, fs::Permissions::from_mode(0o700))
                    .map_err(|_| "无法恢复内核执行权限")?;
            }
            #[cfg(not(unix))]
            let _ = role;
        }
        fs::remove_dir_all(&self.root).map_err(|_| "旧内核已恢复，但恢复目录清理失败")?;
        Ok(())
    }
}
pub(crate) fn recover_pending(paths: &AppPaths) -> Result<(), String> {
    let root = paths
        .directory(DirectoryKind::Core)
        .join(".core-update-recovery");
    if !root.exists() {
        return Ok(());
    }
    if fs::symlink_metadata(&root)
        .map_err(|_| "无法检查更新恢复目录")?
        .file_type()
        .is_symlink()
    {
        return Err("更新恢复目录不能为符号链接".into());
    }
    let bytes =
        read(&root.join("manifest.json"))?.ok_or("更新恢复目录不完整，请检查应用内核目录")?;
    if bytes.len() > 64 * 1024 {
        return Err("更新恢复清单超限".into());
    }
    let manifest: Manifest = serde_json::from_slice(&bytes).map_err(|_| "更新恢复清单格式无效")?;
    if manifest.version != 2 {
        return Err("更新恢复记录版本不受支持".into());
    }
    if manifest.committed {
        fs::remove_dir_all(root).map_err(|_| "无法清理已提交的更新材料")?;
        return Ok(());
    }
    Recovery {
        root,
        paths: paths.clone(),
        manifest,
    }
    .restore()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn interrupted_update_restores_exact_owned_files_and_commit_preserves_new_binary() {
        let root = tempfile::tempdir().unwrap();
        let paths = AppPaths::isolated(root.path());
        paths.create().unwrap();
        let binary = paths
            .directory(DirectoryKind::Core)
            .join(crate::bootstrap::core_name());
        fs::write(&binary, b"old core").unwrap();
        let [native, app] = super::super::Preferences::paths(&paths);
        fs::write(&native, b"old native").unwrap();
        let recovery = Recovery::begin(&paths).unwrap();
        fs::write(&binary, b"new core").unwrap();
        fs::write(&native, b"new native").unwrap();
        crate::preferences::app::write(&paths, "core", &serde_json::json!({"start_core":false}))
            .unwrap();
        drop(recovery);
        recover_pending(&paths).unwrap();
        assert_eq!(fs::read(&binary).unwrap(), b"old core");
        assert_eq!(fs::read(&native).unwrap(), b"old native");
        assert!(
            serde_json::from_slice::<serde_json::Value>(&fs::read(&app).unwrap())
                .unwrap()
                .get("core")
                .is_none()
        );
        let mut recovery = Recovery::begin(&paths).unwrap();
        fs::write(&binary, b"new core").unwrap();
        recovery.commit().unwrap();
        recover_pending(&paths).unwrap();
        assert_eq!(fs::read(binary).unwrap(), b"new core");
    }
    #[test]
    fn corrupt_backup_is_detected_before_any_destination_changes() {
        let root = tempfile::tempdir().unwrap();
        let paths = AppPaths::isolated(root.path());
        paths.create().unwrap();
        let binary = paths
            .directory(DirectoryKind::Core)
            .join(crate::bootstrap::core_name());
        fs::write(&binary, b"old core").unwrap();
        let recovery = Recovery::begin(&paths).unwrap();
        fs::write(recovery.root.join("binary"), b"corrupt").unwrap();
        fs::write(&binary, b"new core").unwrap();
        assert!(recovery.restore().is_err());
        assert_eq!(fs::read(binary).unwrap(), b"new core");
    }
}
