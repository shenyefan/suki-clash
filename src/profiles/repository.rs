//! Profile mutations are committed by the lifecycle actor; metadata and bodies share a recovery plan.
use super::{
    model::{Document, DocumentKind},
    store,
};
use crate::storage::{AppPaths, Namespace, Repository, Values, atomic_write};
use std::{collections::BTreeSet, path::PathBuf};
pub(crate) fn namespace() -> Result<Namespace, String> {
    Namespace::parse("profile-index").map_err(|error| error.to_string())
}
pub(crate) fn read(paths: &AppPaths) -> Result<(Values, Vec<Document>), String> {
    let repository = Repository::open(paths.clone()).map_err(|_| "无法读取订阅目录")?;
    let mut values = repository
        .read_limited(&namespace()?, crate::configuration::INPUT_LIMIT)
        .map_err(|_| "无法读取订阅索引")?;
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
        let id = id.parse::<u64>().map_err(|_| "订阅 ID 无效")?;
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
            .map_err(|_| "订阅超时无效")?;
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
    .map_err(|_| "订阅 YAML 无效或超过大小上限")?;
    Ok(())
}
pub(crate) fn body_path(paths: &AppPaths, id: u64) -> PathBuf {
    paths.data().join("profiles").join(format!("{id}.yaml"))
}
pub(crate) struct Recovery {
    paths: AppPaths,
    runtime: crate::preferences::app::Sections,
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
            targets.push(paths.records().join(format!("profile-enhance-{id}.json")));
        }
        targets.push(paths.records().join("profile-index.json"));
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
                Err(_) => return Err("无法检查订阅恢复材料".into()),
            }
            let content = match std::fs::read(&path) {
                Ok(content) => Some(content),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
                Err(_) => return Err("无法读取订阅恢复材料".into()),
            };
            files.push((path, content));
        }
        Ok(Self {
            paths: paths.clone(),
            runtime: crate::preferences::app::capture(paths, &["runtime"])?,
            files,
        })
    }
    pub(crate) fn restore(&self) -> Result<(), String> {
        let mut errors = Vec::new();
        if let Err(error) = crate::preferences::app::restore(&self.paths, &self.runtime) {
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
    let repository = Repository::open(paths.clone()).map_err(|_| "无法打开订阅存储")?;
    let values = store::values(new, None);
    if serde_json::to_vec(&values)
        .map_err(|_| "无法编码订阅")?
        .len() as u64
        > crate::configuration::INPUT_LIMIT
    {
        return Err("订阅索引超过大小上限".into());
    }
    for document in new {
        atomic_write(&body_path(paths, document.id), document.content.as_bytes())
            .map_err(|_| "订阅文件保存失败")?;
    }
    repository
        .save(&namespace()?, &values)
        .map_err(|_| "订阅索引保存失败")?;
    crate::configuration::select_profile(paths, selection)?;
    for document in old.iter().filter(|doc| !ids.contains(&doc.id)) {
        for path in [
            body_path(paths, document.id),
            paths
                .records()
                .join(format!("profile-enhance-{}.json", document.id)),
        ] {
            match std::fs::remove_file(path) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(_) => return Err("订阅文件删除失败".into()),
            }
        }
    }
    Ok(values)
}
