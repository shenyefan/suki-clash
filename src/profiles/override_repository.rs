//! Current override index and body files; the lifecycle actor owns commits.
use super::{
    model::{Document, DocumentKind},
    store,
};
use crate::storage::{AppPaths, Namespace, Repository, Values, atomic_write};
use std::{collections::BTreeSet, path::PathBuf};
pub(crate) fn read(paths: &AppPaths) -> Result<(Values, Vec<Document>), String> {
    let repository = Repository::open(paths.clone()).map_err(crate::logging::error_message)?;
    let mut values = repository
        .read_limited(
            &Namespace::parse("override-index").map_err(crate::logging::error_message)?,
            crate::configuration::INPUT_LIMIT,
        )
        .map_err(crate::logging::error_message)?;
    store::hydrate(paths, true, &mut values)?;
    crate::configuration::validate_override_index(&values)
        .map_err(crate::logging::error_message)?;
    if values.is_empty() {
        return Ok((values, vec![]));
    }
    let docs = store::load_values(values.clone()).ok_or("覆写索引无效")?.0;
    for doc in &docs {
        validate(doc)?;
    }
    Ok((values, docs))
}
pub(crate) fn validate(document: &Document) -> Result<(), String> {
    if document.id == 0 || document.kind == DocumentKind::Profile || document.name.trim().is_empty()
    {
        return Err("覆写身份、名称或类型无效".into());
    }
    if document.is_remote() {
        if !super::model::valid_remote_url(&document.url) {
            return Err("覆写地址无效".into());
        }
        if !document
            .timeout
            .parse::<u64>()
            .is_ok_and(|v| (1..=3600).contains(&v))
        {
            return Err("下载超时需为 1–3600 秒".into());
        }
        if !document.interval.parse::<u64>().is_ok_and(|v| v <= 525600) {
            return Err("更新间隔需为 0–525600 分钟".into());
        }
    }
    crate::configuration::validate_override_index(&store::values(
        std::slice::from_ref(document),
        None,
    ))
    .map_err(crate::logging::error_message)
}
pub(crate) fn path(paths: &AppPaths, document: &Document) -> PathBuf {
    paths
        .data()
        .join("overrides")
        .join(format!("{}.{}", document.id, document.kind.extension()))
}
pub(crate) struct Recovery(Vec<(PathBuf, Option<Vec<u8>>)>);
impl Recovery {
    pub(crate) fn capture(
        paths: &AppPaths,
        old: &[Document],
        new: &[Document],
    ) -> Result<Self, String> {
        let mut targets: BTreeSet<_> = old.iter().chain(new).map(|doc| path(paths, doc)).collect();
        targets.insert(paths.records().join("override-index.json"));
        let mut entries = vec![];
        for path in targets {
            if let Ok(metadata) = std::fs::symlink_metadata(&path)
                && (!metadata.is_file()
                    || metadata.file_type().is_symlink()
                    || metadata.len() > crate::configuration::INPUT_LIMIT)
            {
                return Err("覆写恢复文件无效或超过上限".into());
            }
            let bytes = match std::fs::read(&path) {
                Ok(bytes) => Some(bytes),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
                Err(error) => {
                    return Err(crate::logging::diagnostic_error(
                        "无法读取覆写恢复材料".into(),
                        error,
                    ));
                }
            };
            entries.push((path, bytes));
        }
        Ok(Self(entries))
    }
    pub(crate) fn restore(&self) -> Result<(), String> {
        let mut failed = 0;
        for (path, bytes) in &self.0 {
            let result = match bytes {
                Some(bytes) => atomic_write(path, bytes),
                None => match std::fs::remove_file(path) {
                    Ok(()) => Ok(()),
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
                    Err(error) => Err(error.into()),
                },
            };
            if result.is_err() {
                failed += 1;
            }
        }
        if failed == 0 {
            Ok(())
        } else {
            Err(format!("{failed} 项覆写文件恢复失败"))
        }
    }
}
pub(crate) fn commit(
    paths: &AppPaths,
    old: &[Document],
    new: &[Document],
) -> Result<Values, String> {
    let mut ids = BTreeSet::new();
    for document in new {
        validate(document)?;
        if !ids.insert(document.id) {
            return Err("覆写 ID 重复".into());
        }
    }
    let values = store::metadata_values(new);
    if serde_json::to_vec(&values)
        .map_err(|error| crate::logging::diagnostic_error("无法编码覆写索引", error))?
        .len() as u64
        > crate::configuration::INPUT_LIMIT
    {
        return Err("覆写索引超过上限".into());
    }
    for document in new {
        if old
            .iter()
            .find(|old| old.id == document.id && old.kind == document.kind)
            .is_none_or(|old| old.content != document.content)
        {
            atomic_write(&path(paths, document), document.content.as_bytes())
                .map_err(|error| crate::logging::diagnostic_error("覆写文件保存失败", error))?;
        }
    }
    Repository::open(paths.clone())
        .map_err(crate::logging::error_message)?
        .save(
            &Namespace::parse("override-index").map_err(crate::logging::error_message)?,
            &values,
        )
        .map_err(crate::logging::error_message)?;
    let retained: BTreeSet<_> = new.iter().map(|doc| path(paths, doc)).collect();
    for document in old {
        let path = path(paths, document);
        if !retained.contains(&path) {
            match std::fs::remove_file(path) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => {
                    return Err(crate::logging::diagnostic_error(
                        "覆写文件删除失败".into(),
                        error,
                    ));
                }
            }
        }
    }
    Ok(store::values(new, None))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn external_override_body_updates_composition_without_being_reexported() {
        let root = tempfile::tempdir().unwrap();
        let paths = AppPaths::isolated(root.path());
        let repository = Repository::open(paths.clone()).unwrap();
        let mut document = Document::new(9, DocumentKind::Yaml);
        document.name = "override".into();
        document.enabled = true;
        document.content = "mode: rule\n".into();
        commit(&paths, &[], &[document.clone()]).unwrap();
        let index = repository
            .read(&Namespace::parse("override-index").unwrap())
            .unwrap();
        assert!(!index.contains_key("9/content"));
        let body = path(&paths, &document);
        atomic_write(&body, b"mode: direct\n").unwrap();
        let (values, old) = read(&paths).unwrap();
        let output = crate::configuration::compose(b"{}", None, &values).unwrap();
        let output: serde_yaml_ng::Value = serde_yaml_ng::from_slice(&output).unwrap();
        assert_eq!(output["mode"], "direct");
        let mut renamed = old.clone();
        renamed[0].name = "renamed".into();
        atomic_write(&body, b"mode: global\n").unwrap();
        commit(&paths, &old, &renamed).unwrap();
        assert_eq!(std::fs::read(&body).unwrap(), b"mode: global\n");
        std::fs::remove_file(&body).unwrap();
        assert!(read(&paths).err().unwrap().contains("9.yaml"));
    }

    #[test]
    fn inline_override_body_reports_the_index_without_rewriting_either_file() {
        let root = tempfile::tempdir().unwrap();
        let paths = AppPaths::isolated(root.path());
        let repository = Repository::open(paths.clone()).unwrap();
        let mut document = Document::new(9, DocumentKind::Yaml);
        document.name = "test override".into();
        document.content = "mode: direct\n".into();
        let values = store::values(std::slice::from_ref(&document), None);
        let index = Namespace::parse("override-index").unwrap();
        repository.save(&index, &values).unwrap();
        atomic_write(&path(&paths, &document), document.content.as_bytes()).unwrap();
        let original = std::fs::read(paths.records().join("override-index.json")).unwrap();
        let error = read(&paths).err().unwrap();
        assert!(error.contains("override-index.json"));
        assert!(error.contains("保留独立正文文件"));
        assert!(!error.contains("mode: direct"));
        assert_eq!(
            std::fs::read(paths.records().join("override-index.json")).unwrap(),
            original
        );
        assert_eq!(
            std::fs::read(path(&paths, &document)).unwrap(),
            document.content.as_bytes()
        );
    }
}
