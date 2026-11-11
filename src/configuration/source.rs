//! Active subscription identity is persisted; every start reloads the current source.
use crate::storage::{AppPaths, Namespace, Repository};
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ProfileIdentity {
    id: u64,
    name: String,
    revision: String,
}
impl ProfileIdentity {
    pub(crate) fn id(&self) -> u64 {
        self.id
    }
    pub(crate) fn name(&self) -> &str {
        &self.name
    }
}
#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct Source {
    profile: Option<ProfileIdentity>,
    content: String,
}
pub(crate) fn initialize_selection(paths: &AppPaths) -> Result<(), String> {
    let selection = crate::profiles::settings::Settings::load(paths)?;
    selection.save(paths)
}
pub(crate) fn selected_profile(paths: &AppPaths) -> Result<Option<u64>, String> {
    Ok(crate::profiles::settings::Settings::load(paths)?.selected_profile_id)
}
pub(crate) fn select_profile(paths: &AppPaths, id: Option<u64>) -> Result<(), String> {
    let mut settings = crate::profiles::settings::Settings::load(paths)?;
    settings.selected_profile_id = id;
    settings.save(paths)
}
impl Source {
    pub(crate) fn base() -> Self {
        Self {
            profile: None,
            content: "{}\n".into(),
        }
    }
    pub(crate) fn profile(&self) -> Option<&ProfileIdentity> {
        self.profile.as_ref()
    }
    pub(crate) fn content(&self) -> &[u8] {
        self.content.as_bytes()
    }

    pub(crate) fn from_profile(paths: &AppPaths, id: u64) -> Result<Self, String> {
        let repository = Repository::open(paths.clone())
            .map_err(|error| crate::logging::diagnostic_error("无法读取订阅存储", error))?;
        let mut values = repository
            .read_limited(
                &Namespace::parse("profile-index").map_err(crate::logging::error_message)?,
                super::INPUT_LIMIT,
            )
            .map_err(crate::logging::error_message)?;
        crate::profiles::store::validate_metadata(paths, false, &values)?;
        let content =
            crate::profiles::store::read_body(&crate::profiles::repository::body_path(paths, id))?;
        values.insert(format!("{id}/content"), content);
        Self::from_profile_values(&values, id)
    }
    pub(crate) fn from_profile_values(
        values: &crate::storage::Values,
        id: u64,
    ) -> Result<Self, String> {
        let ids = values.get("ids").ok_or("暂无保存的订阅")?;
        let parsed = ids
            .split(',')
            .map(str::parse::<u64>)
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| crate::logging::rejected("订阅索引 ID 无效", error))?;
        if values.get("format").map(String::as_str) != Some("1")
            || parsed.iter().filter(|value| **value == id).count() != 1
        {
            return Err("订阅不存在或索引已失效".into());
        }
        let field = |key| values.get(&format!("{id}/{key}")).map(String::as_str);
        if field("kind") != Some("配置") {
            return Err("该条目不是可激活的订阅来源".into());
        }
        let content = field("content")
            .filter(|s| !s.trim().is_empty())
            .ok_or("订阅尚无已下载内容，请先导入有效配置")?
            .to_owned();
        if content.len() as u64 > super::INPUT_LIMIT {
            return Err("订阅内容超过大小上限".into());
        }
        let profile = ProfileIdentity {
            id,
            name: field("name").unwrap_or("未命名订阅").into(),
            revision: Sha256::digest(content.as_bytes())
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>(),
        };
        Ok(Self {
            profile: Some(profile),
            content,
        })
    }

    pub(crate) async fn load(paths: &AppPaths) -> Result<Self, String> {
        initialize_selection(paths)?;
        let selection = crate::profiles::settings::Settings::load(paths)?;
        if let Some(id) = selection.selected_profile_id {
            return Self::from_profile(paths, id);
        }
        Self::basic(paths)
    }

    /// Like CV's empty current profile plus application base settings: this is
    /// an actual minimal runtime, with no invented subscription or proxy nodes.
    pub(crate) fn basic(paths: &AppPaths) -> Result<Self, String> {
        let preferences = super::Preferences::load(paths)?;
        let mut mapping = serde_yaml_ng::Mapping::new();
        preferences.overlay(&mut mapping);
        mapping.insert("mode".into(), "rule".into());
        let content = serde_yaml_ng::to_string(&mapping)
            .map_err(|error| crate::logging::diagnostic_error("无法生成基础配置", error))?;
        Ok(Self {
            profile: None,
            content,
        })
    }

    /// Commit only the selected subscription after the core confirms loading.
    pub(crate) fn commit(&self, paths: &AppPaths) -> Result<(), String> {
        select_profile(paths, self.profile.as_ref().map(ProfileIdentity::id))
    }
}
