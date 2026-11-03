//! Local archive files; configuration capture and restoration remain lifecycle-owned.
use crate::storage::{AppPaths, DirectoryKind};
use std::{
    fs,
    io::{Read as _, Write as _},
    path::{Path, PathBuf},
    time::SystemTime,
};

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct Id(String);
impl Id {
    pub(crate) fn name(&self) -> &str {
        &self.0
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Entry {
    pub(crate) id: Id,
    pub(crate) modified: SystemTime,
    pub(crate) size: u64,
}
fn directory(paths: &AppPaths) -> PathBuf {
    paths.directory(DirectoryKind::Backups).join("local")
}
fn file(paths: &AppPaths, id: &Id) -> PathBuf {
    directory(paths).join(&id.0)
}
pub(crate) fn list(paths: &AppPaths) -> Result<Vec<Entry>, String> {
    let directory = directory(paths);
    let files = match fs::read_dir(&directory) {
        Ok(files) => files,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(format!("{}：{error}", directory.display())),
    };
    let mut entries = Vec::new();
    for entry in files {
        let entry = entry.map_err(|error| error.to_string())?;
        let name = entry.file_name();
        let Some(name) = name
            .to_str()
            .filter(|name| name.starts_with("local-") && name.ends_with(".suki-backup"))
        else {
            continue;
        };
        let metadata = fs::symlink_metadata(entry.path()).map_err(|error| error.to_string())?;
        if !metadata.is_file() {
            return Err(format!("{name}：备份不是普通文件"));
        }
        entries.push(Entry {
            id: Id(name.into()),
            modified: metadata.modified().map_err(|error| error.to_string())?,
            size: metadata.len(),
        });
    }
    entries.sort_by(|a, b| b.modified.cmp(&a.modified).then(a.id.cmp(&b.id)));
    Ok(entries)
}
pub(crate) fn store(paths: &AppPaths, content: &str) -> Result<Id, String> {
    let root = directory(paths);
    fs::create_dir_all(&root).map_err(|error| error.to_string())?;
    let mut random = [0u8; 8];
    getrandom::fill(&mut random).map_err(|error| error.to_string())?;
    let random = random
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    let id = Id(format!(
        "local-{}-{random}.suki-backup",
        chrono::Utc::now().format("%Y%m%dT%H%M%SZ")
    ));
    let mut temporary =
        tempfile::NamedTempFile::new_in(&root).map_err(|error| error.to_string())?;
    temporary
        .write_all(content.as_bytes())
        .map_err(|error| error.to_string())?;
    temporary
        .as_file()
        .sync_all()
        .map_err(|error| error.to_string())?;
    temporary
        .persist_noclobber(file(paths, &id))
        .map_err(|error| error.to_string())?;
    Ok(id)
}
pub(crate) fn read_path(path: &Path) -> Result<String, String> {
    let metadata =
        fs::symlink_metadata(path).map_err(|error| format!("{}：{error}", path.display()))?;
    if !metadata.is_file() || metadata.len() > super::ENCODED_LIMIT as u64 {
        return Err("备份必须是48MiB以内的普通文件".into());
    }
    let mut bytes = Vec::new();
    fs::File::open(path)
        .map_err(|error| error.to_string())?
        .take(super::ENCODED_LIMIT as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    if bytes.len() > super::ENCODED_LIMIT {
        return Err("备份超过48MiB上限".into());
    }
    String::from_utf8(bytes).map_err(|_| "备份不是UTF-8文本".into())
}
pub(crate) fn read(paths: &AppPaths, id: &Id) -> Result<String, String> {
    read_path(&file(paths, id))
}
pub(crate) fn delete(paths: &AppPaths, id: &Id) -> Result<(), String> {
    let target = file(paths, id);
    if !fs::symlink_metadata(&target)
        .map_err(|error| error.to_string())?
        .is_file()
    {
        return Err("备份不是普通文件".into());
    }
    fs::remove_file(target).map_err(|error| error.to_string())
}
pub(crate) fn export(paths: &AppPaths, id: &Id, target: &Path) -> Result<(), String> {
    if target == file(paths, id) {
        return Err("导出目标不能是当前备份文件".into());
    }
    crate::storage::atomic_write(target, read(paths, id)?.as_bytes())
        .map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn local_history_export_and_delete_preserve_independent_files() {
        let root = tempfile::tempdir().unwrap();
        let paths = AppPaths::isolated(root.path());
        paths.create().unwrap();
        let a = store(&paths, "archive a").unwrap();
        let b = store(&paths, "archive b").unwrap();
        assert_ne!(a, b);
        assert_eq!(list(&paths).unwrap().len(), 2);
        let exported = root.path().join("export.suki-backup");
        export(&paths, &a, &exported).unwrap();
        assert_eq!(read_path(&exported).unwrap(), "archive a");
        delete(&paths, &a).unwrap();
        assert_eq!(list(&paths).unwrap()[0].id, b);
        assert!(exported.exists());
    }
    #[cfg(unix)]
    #[test]
    fn external_symlinks_and_non_archive_files_do_not_become_backups() {
        let root = tempfile::tempdir().unwrap();
        let paths = AppPaths::isolated(root.path());
        paths.create().unwrap();
        let source = root.path().join("source");
        fs::write(&source, "not an archive").unwrap();
        let link = root.path().join("link");
        std::os::unix::fs::symlink(&source, &link).unwrap();
        assert!(read_path(&link).is_err());
        assert!(super::super::Archive::decode(&read_path(&source).unwrap(), None).is_err());
        assert!(list(&paths).unwrap().is_empty());
    }
}
