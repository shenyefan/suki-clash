//! Profile mutations are committed by the lifecycle actor; metadata and bodies share a recovery plan.
use super::{
    model::{Document, DocumentKind},
    store,
};
use crate::storage::{AppPaths, Namespace, Repository, Values, atomic_write};
use std::{collections::BTreeSet, path::PathBuf};
pub(crate) fn namespace() -> Result<Namespace, String> {
    Namespace::parse("profile-index").map_err(crate::logging::error_message)
}
pub(crate) fn read(paths: &AppPaths) -> Result<(Values, Vec<Document>), String> {
    let repository = Repository::open(paths.clone())
        .map_err(|error| crate::logging::diagnostic_error("无法读取订阅目录", error))?;
    let mut values = repository
        .read_limited(&namespace()?, crate::configuration::INPUT_LIMIT)
        .map_err(|error| crate::logging::diagnostic_error("无法读取订阅索引", error))?;
    store::hydrate(paths, false, &mut values)?;
    let documents = decode(&values)?;
    values.remove("active");
    if let Some(id) = crate::configuration::selected_profile(paths)? {
        values.insert("active".into(), id.to_string());
    }
    Ok((values, documents))
}
pub(crate) fn decode(values: &Values) -> Result<Vec<Document>, String> {
    if values.is_empty() {
        return Ok(vec![]);
    }
    if values.get("format").map(String::as_str) != Some("1") {
        return Err("订阅索引格式无效".into());
    }
    let ids = values.get("ids").ok_or("订阅索引缺少列表")?;
    let mut unique = BTreeSet::new();
    for id in ids.split(',').filter(|id| !id.is_empty()) {
        let id = id
            .parse::<u64>()
            .map_err(|error| crate::logging::rejected("订阅 ID 无效", error))?;
        if id == 0 || !unique.insert(id) {
            return Err("订阅 ID 重复或无效".into());
        }
        if values.get(&format!("{id}/kind")).map(String::as_str) != Some("配置") {
            return Err("订阅类型无效".into());
        }
    }
    let documents = store::load_values(values.clone()).ok_or("订阅索引无效")?.0;
    if documents.len() != unique.len() {
        return Err("订阅索引不完整".into());
    }
    Ok(documents)
}
pub(crate) fn validate(document: &Document) -> Result<(), String> {
    if document.id == 0 || document.kind != DocumentKind::Profile || document.name.trim().is_empty()
    {
        return Err("订阅名称或身份无效".into());
    }
    if document.token.len() > 8192
        || document.user_agent.len() > 8192
        || document.age_key.len() > 512
    {
        return Err("订阅请求参数超过长度上限".into());
    }
    if document.is_remote() {
        if !super::model::valid_remote_url(&document.url) {
            return Err("订阅地址无效".into());
        }
        let timeout = document
            .timeout
            .parse::<u64>()
            .map_err(|error| crate::logging::rejected("订阅超时无效", error))?;
        if !(1..=3600).contains(&timeout) {
            return Err("订阅超时需为 1–3600 秒".into());
        }
        if document
            .interval
            .parse::<u64>()
            .ok()
            .filter(|value| *value <= 525600)
            .is_none()
        {
            return Err("订阅更新间隔需为 0–525600 分钟".into());
        }
    }
    crate::configuration::compose(
        document.content.as_bytes(),
        Some(document.id),
        &Values::new(),
    )
    .map_err(|error| crate::logging::rejected("订阅 YAML 无效或超过大小上限", error))?;
    Ok(())
}
pub(crate) fn body_path(paths: &AppPaths, id: u64) -> PathBuf {
    paths.data().join("profiles").join(format!("{id}.yaml"))
}
pub(crate) struct Recovery {
    paths: AppPaths,
    settings: crate::preferences::app::Sections,
    files: Vec<(PathBuf, Option<Vec<u8>>)>,
}
impl Recovery {
    pub(crate) fn capture(
        paths: &AppPaths,
        old: &[Document],
        new: &[Document],
    ) -> Result<Self, String> {
        let mut files = Vec::new();
        let ids: BTreeSet<_> = old.iter().chain(new).map(|doc| doc.id).collect();
        let mut targets = Vec::new();
        for id in ids {
            targets.push(body_path(paths, id));
            targets.push(paths.indexes().join(format!("profile-enhance-{id}.json")));
        }
        for path in targets {
            match std::fs::symlink_metadata(&path) {
                Ok(metadata)
                    if !metadata.is_file()
                        || metadata.file_type().is_symlink()
                        || metadata.len() > crate::configuration::INPUT_LIMIT =>
                {
                    return Err("订阅恢复文件无效或超过大小上限".into());
                }
                Ok(_) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => {
                    return Err(crate::logging::diagnostic_error(
                        "无法检查订阅恢复材料".into(),
                        error,
                    ));
                }
            }
            let content = match std::fs::read(&path) {
                Ok(content) => Some(content),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
                Err(error) => {
                    return Err(crate::logging::diagnostic_error(
                        "无法读取订阅恢复材料".into(),
                        error,
                    ));
                }
            };
            files.push((path, content));
        }
        Ok(Self {
            paths: paths.clone(),
            settings: crate::preferences::app::capture(paths, &["profiles"])?,
            files,
        })
    }
    pub(crate) fn restore(&self) -> Result<(), String> {
        let mut errors = Vec::new();
        if let Err(error) = crate::preferences::app::restore(&self.paths, &self.settings) {
            errors.push(error);
        }
        for (path, content) in &self.files {
            let result = match content {
                Some(bytes) => atomic_write(path, bytes),
                None => match std::fs::remove_file(path) {
                    Ok(()) => Ok(()),
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
                    Err(error) => Err(error.into()),
                },
            };
            if result.is_err() {
                errors.push(
                    path.file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .into_owned(),
                );
            }
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(format!("订阅文件恢复失败：{}", errors.join("、")))
        }
    }
}
pub(crate) fn commit(
    paths: &AppPaths,
    old: &[Document],
    new: &[Document],
    selection: Option<u64>,
) -> Result<Values, String> {
    let mut ids = BTreeSet::new();
    for document in new {
        validate(document)?;
        if !ids.insert(document.id) {
            return Err("订阅 ID 重复".into());
        }
    }
    let repository = Repository::open(paths.clone())
        .map_err(|error| crate::logging::diagnostic_error("无法打开订阅存储", error))?;
    let values = store::metadata_values(new);
    if serde_json::to_vec(&values)
        .map_err(|error| crate::logging::diagnostic_error("无法编码订阅", error))?
        .len() as u64
        > crate::configuration::INPUT_LIMIT
    {
        return Err("订阅索引超过大小上限".into());
    }
    for document in new {
        if old
            .iter()
            .find(|old| old.id == document.id)
            .is_none_or(|old| old.content != document.content)
        {
            atomic_write(&body_path(paths, document.id), document.content.as_bytes())
                .map_err(|error| crate::logging::diagnostic_error("订阅文件保存失败", error))?;
        }
    }
    repository
        .save(&namespace()?, &values)
        .map_err(|error| crate::logging::diagnostic_error("订阅索引保存失败", error))?;
    crate::configuration::select_profile(paths, selection)?;
    for document in old.iter().filter(|doc| !ids.contains(&doc.id)) {
        for path in [
            body_path(paths, document.id),
            paths
                .indexes()
                .join(format!("profile-enhance-{}.json", document.id)),
        ] {
            match std::fs::remove_file(path) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => {
                    return Err(crate::logging::diagnostic_error(
                        "订阅文件删除失败".into(),
                        error,
                    ));
                }
            }
        }
    }
    Ok(store::values(new, selection))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn external_body_is_the_only_source_and_metadata_edits_do_not_rewrite_it() {
        let root = tempfile::tempdir().unwrap();
        let paths = AppPaths::isolated(root.path());
        let repository = Repository::open(paths.clone()).unwrap();
        let mut document = Document::new(7, DocumentKind::Profile);
        document.name = "local".into();
        document.content = "mode: rule\n".into();
        commit(&paths, &[], &[document], None).unwrap();
        assert!(
            !repository
                .read(&namespace().unwrap())
                .unwrap()
                .contains_key("7/content")
        );
        let body = body_path(&paths, 7);
        atomic_write(&body, b"mode: direct\n").unwrap();
        let (revision, old) = read(&paths).unwrap();
        assert_eq!(revision["7/content"], "mode: direct\n");
        assert_eq!(
            crate::configuration::Source::from_profile(&paths, 7)
                .unwrap()
                .content(),
            b"mode: direct\n"
        );
        let mut renamed = old.clone();
        renamed[0].name = "renamed".into();
        // A metadata-only commit must not export its earlier body snapshot back
        // over a file saved by the external editor.
        atomic_write(&body, b"mode: global\n").unwrap();
        commit(&paths, &old, &renamed, None).unwrap();
        assert_eq!(std::fs::read(&body).unwrap(), b"mode: global\n");
        assert_eq!(read(&paths).unwrap().1[0].name, "renamed");
        let mut metadata = repository.read(&namespace().unwrap()).unwrap();
        metadata.insert("ids".into(), "7,8".into());
        metadata.insert("8/kind".into(), "配置".into());
        metadata.insert("8/name".into(), "missing unrelated body".into());
        repository.save(&namespace().unwrap(), &metadata).unwrap();
        assert_eq!(
            crate::configuration::Source::from_profile(&paths, 7)
                .unwrap()
                .content(),
            b"mode: global\n"
        );
        std::fs::remove_file(&body).unwrap();
        assert!(read(&paths).err().unwrap().contains("7.yaml"));
    }

    #[test]
    fn inline_index_content_is_rejected_before_writing_settings() {
        let root = tempfile::tempdir().unwrap();
        let paths = AppPaths::isolated(root.path());
        let repository = Repository::open(paths.clone()).unwrap();
        let error = repository
            .save(
                &namespace().unwrap(),
                &Values::from([
                    ("format".into(), "1".into()),
                    ("ids".into(), "7".into()),
                    ("7/kind".into(), "配置".into()),
                    ("7/content".into(), "mode: rule".into()),
                ]),
            )
            .unwrap_err();
        assert!(error.to_string().contains("索引不得包含正文"));
        assert!(!crate::preferences::app::path(&paths).exists());
        assert!(!body_path(&paths, 7).exists());
    }
}
