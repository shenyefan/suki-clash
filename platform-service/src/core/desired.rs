use crate::core::auth::{AuthenticatedOwner, hash_session_token};
use crate::core::logger::set_or_update_writer;
use crate::core::manager::CORE_MANAGER;
use crate::core::paths::service_paths;
use crate::core::state::set_core_lifecycle_state;
use crate::{ClashConfig, OwnerIdentity, ServiceLifecycleState, WriterConfig};
use anyhow::{Context, Result};
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::io::AsyncWriteExt as _;
use tokio::sync::Mutex;
use tracing::{info, warn};

static DESIRED_STATE_LOCK: Lazy<Mutex<()>> = Lazy::new(|| Mutex::new(()));
static STATE_FILE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DesiredState {
    pub core_should_be_running: bool,
    pub last_clash_config: Option<ClashConfig>,
    pub last_writer_config: Option<WriterConfig>,
    pub generation: u64,
    pub updated_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ActiveOwnerState {
    pub owner_key: String,
    pub identity: OwnerIdentity,
    pub app_data_root: String,
    pub generation: u64,
    pub session_token_hash: String,
}

#[cfg(test)]
impl From<&AuthenticatedOwner> for ActiveOwnerState {
    fn from(owner: &AuthenticatedOwner) -> Self {
        Self {
            owner_key: owner.key.clone(),
            identity: owner.identity.clone(),
            app_data_root: owner.app_data_root.to_string_lossy().into_owned(),
            generation: 0,
            session_token_hash: String::new(),
        }
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct OwnerGenerationState {
    generation: u64,
}

pub async fn load_owner_desired_state(owner_key: &str) -> Result<DesiredState> {
    let path = service_paths()?
        .for_owner_key(owner_key)
        .desired_state_path();
    read_json_or_default(&path).await
}

pub async fn persist_owner_core_started(
    owner: &AuthenticatedOwner,
    config: &ClashConfig,
) -> Result<DesiredState> {
    update_owner_desired_state(&owner.key, |state| {
        state.core_should_be_running = true;
        state.last_clash_config = Some(config.clone());
        state.last_writer_config = Some(config.log_config.clone());
    })
    .await
}

pub async fn persist_owner_core_stopped(owner: &AuthenticatedOwner) -> Result<DesiredState> {
    persist_owner_core_stopped_by_key(&owner.key).await
}

pub async fn persist_owner_core_stopped_by_key(owner_key: &str) -> Result<DesiredState> {
    update_owner_desired_state(owner_key, |state| {
        state.core_should_be_running = false;
    })
    .await
}

pub async fn persist_owner_writer_config(
    owner: &AuthenticatedOwner,
    config: &WriterConfig,
) -> Result<DesiredState> {
    update_owner_desired_state(&owner.key, |state| {
        state.last_writer_config = Some(config.clone());
        if let Some(clash_config) = state.last_clash_config.as_mut() {
            clash_config.log_config = config.clone();
        }
    })
    .await
}

pub async fn load_active_owner() -> Result<Option<ActiveOwnerState>> {
    let path = service_paths()?.active_owner_path();
    secure_state_file_if_exists(&path)?;
    match tokio::fs::read(&path).await {
        Ok(content) => serde_json::from_slice(&content)
            .map(Some)
            .with_context(|| format!("failed to parse active owner {path:?}")),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error).with_context(|| format!("failed to read active owner {path:?}")),
    }
}

#[cfg(test)]
pub async fn persist_active_owner(owner: &AuthenticatedOwner) -> Result<ActiveOwnerState> {
    let _guard = DESIRED_STATE_LOCK.lock().await;
    let state = ActiveOwnerState::from(owner);
    write_json_atomic(&service_paths()?.active_owner_path(), &state).await?;
    Ok(state)
}

pub async fn commit_active_owner_session(
    owner: &AuthenticatedOwner,
    session_token: &str,
) -> Result<ActiveOwnerState> {
    let session_token_hash = hash_session_token(session_token)?;
    let _guard = DESIRED_STATE_LOCK.lock().await;
    let paths = service_paths()?;
    let generation_path = paths.owner_generation_path();
    let mut generation_state: OwnerGenerationState = read_json_or_default(&generation_path).await?;
    generation_state.generation = generation_state.generation.saturating_add(1);
    write_json_atomic(&generation_path, &generation_state).await?;

    let state = ActiveOwnerState {
        owner_key: owner.key.clone(),
        identity: owner.identity.clone(),
        app_data_root: owner.app_data_root.to_string_lossy().into_owned(),
        generation: generation_state.generation,
        session_token_hash,
    };
    write_json_atomic(&paths.active_owner_path(), &state).await?;
    Ok(state)
}

pub async fn clear_active_owner() -> Result<()> {
    let _guard = DESIRED_STATE_LOCK.lock().await;
    let path = service_paths()?.active_owner_path();
    match tokio::fs::remove_file(&path).await {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error).with_context(|| format!("failed to remove active owner {path:?}")),
    }
}

pub async fn restore_desired_state() -> Result<()> {
    let Some(active_owner) = load_active_owner().await? else {
        info!("无活动所有者，跳过恢复");
        return Ok(());
    };
    let state = load_owner_desired_state(&active_owner.owner_key).await?;

    if let Some(writer_config) = state.last_writer_config.as_ref()
        && let Err(error) = set_or_update_writer(writer_config)
    {
        warn!("恢复写入配置失败：{}", error);
    }

    if !state.core_should_be_running {
        info!("期望状态无需恢复内核");
        return Ok(());
    }

    let Some(config) = state.last_clash_config else {
        warn!("期望状态要求恢复内核但缺少内核配置");
        return Ok(());
    };

    info!("按期望状态第 {} 代恢复内核", state.generation);
    if let Err(error) = CORE_MANAGER
        .lock()
        .await
        .start_core(config, active_owner.identity.clone())
        .await
    {
        set_core_lifecycle_state(ServiceLifecycleState::Fatal);
        return Err(error);
    }
    Ok(())
}

async fn update_owner_desired_state(
    owner_key: &str,
    update: impl FnOnce(&mut DesiredState),
) -> Result<DesiredState> {
    let _guard = DESIRED_STATE_LOCK.lock().await;
    let path = service_paths()?
        .for_owner_key(owner_key)
        .desired_state_path();
    let mut state = read_json_or_default(&path).await?;
    update(&mut state);
    state.generation = state.generation.saturating_add(1);
    state.updated_at = unix_timestamp_secs();
    write_json_atomic(&path, &state).await?;
    Ok(state)
}

async fn read_json_or_default<T>(path: &std::path::Path) -> Result<T>
where
    T: for<'de> Deserialize<'de> + Default,
{
    secure_state_file_if_exists(path)?;
    match tokio::fs::read(path).await {
        Ok(content) => serde_json::from_slice(&content)
            .with_context(|| format!("failed to parse state {path:?}")),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(T::default()),
        Err(error) => Err(error).with_context(|| format!("failed to read state {path:?}")),
    }
}

async fn write_json_atomic<T>(path: &std::path::Path, value: &T) -> Result<()>
where
    T: Serialize,
{
    crate::core::paths::ensure_persistent_state_layout()?;
    if let Some(parent) = path.parent() {
        crate::core::platform_security::ensure_private_service_directory(parent)?;
    }

    let temp_path = sibling_state_path(path, "tmp");
    let json = serde_json::to_vec_pretty(value)?;
    let result = async {
        let mut file = tokio::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp_path)
            .await
            .with_context(|| format!("failed to create state temp file {temp_path:?}"))?;
        file.write_all(&json)
            .await
            .with_context(|| format!("failed to write state temp file {temp_path:?}"))?;
        file.flush()
            .await
            .with_context(|| format!("failed to flush state temp file {temp_path:?}"))?;
        file.sync_all()
            .await
            .with_context(|| format!("failed to sync state temp file {temp_path:?}"))?;
        drop(file);
        secure_state_file_if_exists(&temp_path)?;
        crate::core::atomic_file::replace(&temp_path, path)
            .await
            .with_context(|| format!("failed to move state into {path:?}"))?;
        secure_state_file_if_exists(path)?;
        Ok(())
    }
    .await;

    if result.is_err()
        && let Err(cleanup_error) = tokio::fs::remove_file(&temp_path).await
        && cleanup_error.kind() != std::io::ErrorKind::NotFound
    {
        warn!("清理状态临时文件 {:?} 失败：{}", temp_path, cleanup_error);
    }
    result
}

fn sibling_state_path(path: &std::path::Path, role: &str) -> std::path::PathBuf {
    let sequence = STATE_FILE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    let file_name = path.file_name().unwrap_or_default().to_string_lossy();
    path.with_file_name(format!(
        "{file_name}.{role}-{}-{timestamp}-{sequence}",
        std::process::id()
    ))
}

fn secure_state_file_if_exists(path: &std::path::Path) -> Result<()> {
    crate::core::platform_security::secure_private_service_file_if_exists(path)?;
    Ok(())
}

fn unix_timestamp_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default()
}

#[cfg(test)]
mod owner_tests {
    use super::{
        clear_active_owner, commit_active_owner_session, load_active_owner,
        load_owner_desired_state, persist_active_owner, persist_owner_core_started,
        persist_owner_core_stopped,
    };
    use crate::core::auth::AuthenticatedOwner;
    use crate::{ClashConfig, CoreConfig, OwnerIdentity};
    use serial_test::serial;

    fn test_owner(uid: u32) -> AuthenticatedOwner {
        AuthenticatedOwner {
            key: uid.to_string(),
            identity: OwnerIdentity::Unix { uid, gid: 20 },
            app_data_root: std::env::temp_dir(),
        }
    }

    #[tokio::test]
    #[serial]
    async fn unusable_restored_core_does_not_reserve_execution() -> anyhow::Result<()> {
        let owner = test_owner(90_019);
        let directory =
            std::env::temp_dir().join(format!("missing-core-restore-{}", std::process::id()));
        std::fs::create_dir_all(&directory)?;
        let config = ClashConfig {
            core_config: CoreConfig {
                core_path: "/missing/suki-mihomo".into(),
                core_ipc_path: directory.join("core.sock").to_string_lossy().into_owned(),
                ..Default::default()
            },
            log_config: Default::default(),
        };
        persist_owner_core_started(&owner, &config).await?;
        persist_active_owner(&owner).await?;
        assert!(
            crate::core::installation::inspect(&[], false)
                .await?
                .core_busy
        );
        assert!(super::restore_desired_state().await.is_err());
        let status = crate::core::installation::inspect(&[], false).await?;
        let occupied = status.core_busy;
        assert!(status.service_sha256.is_empty());
        clear_active_owner().await?;
        std::fs::remove_dir_all(directory)?;
        assert!(
            occupied,
            "failed restore must preserve intent and block an implicit sidecar handoff"
        );
        Ok(())
    }

    #[tokio::test]
    async fn desired_state_is_scoped_by_owner_key() -> anyhow::Result<()> {
        let owner_a = test_owner(90_001);
        let owner_b = test_owner(90_002);
        let config = ClashConfig {
            core_config: CoreConfig {
                core_path: "/tmp/mock-core-a".to_string(),
                ..Default::default()
            },
            log_config: Default::default(),
        };

        persist_owner_core_started(&owner_a, &config).await?;
        persist_owner_core_stopped(&owner_b).await?;

        assert!(
            load_owner_desired_state(&owner_a.key)
                .await?
                .core_should_be_running
        );
        assert!(
            !load_owner_desired_state(&owner_b.key)
                .await?
                .core_should_be_running
        );
        Ok(())
    }

    #[tokio::test]
    #[serial]
    async fn active_owner_can_be_atomically_replaced_and_cleared() -> anyhow::Result<()> {
        let owner_a = test_owner(90_003);
        let owner_b = test_owner(90_004);

        persist_active_owner(&owner_a).await?;
        assert_eq!(
            load_active_owner()
                .await?
                .as_ref()
                .map(|state| state.owner_key.as_str()),
            Some("90003")
        );

        persist_active_owner(&owner_b).await?;
        assert_eq!(
            load_active_owner()
                .await?
                .as_ref()
                .map(|state| state.owner_key.as_str()),
            Some("90004")
        );

        clear_active_owner().await?;
        assert!(load_active_owner().await?.is_none());
        Ok(())
    }

    #[tokio::test]
    #[serial]
    async fn owner_generation_survives_active_owner_clear() -> anyhow::Result<()> {
        clear_active_owner().await?;
        let owner = test_owner(90_005);
        let first_token = "33".repeat(32);
        let first = commit_active_owner_session(&owner, &first_token).await?;

        assert_ne!(first.session_token_hash, first_token);
        assert!(
            !tokio::fs::read_to_string(crate::service_paths()?.active_owner_path())
                .await?
                .contains(&first_token)
        );

        clear_active_owner().await?;
        let second = commit_active_owner_session(&owner, &"44".repeat(32)).await?;

        assert!(second.generation > first.generation);
        clear_active_owner().await?;
        Ok(())
    }

    #[tokio::test]
    #[serial]
    async fn session_commit_rejects_non_lowercase_hex_tokens() -> anyhow::Result<()> {
        clear_active_owner().await?;
        let owner = test_owner(90_007);

        for invalid in ["55".repeat(31), "AA".repeat(32), "gg".repeat(32)] {
            assert!(commit_active_owner_session(&owner, &invalid).await.is_err());
        }
        assert!(load_active_owner().await?.is_none());
        Ok(())
    }

    #[tokio::test]
    #[serial]
    async fn unavailable_owner_desired_state_rejects_uncommitted_change() -> anyhow::Result<()> {
        let owner = test_owner(90_009);
        let owner_root = crate::service_paths()?
            .for_owner_key(&owner.key)
            .root()
            .to_path_buf();
        let _ = std::fs::remove_dir_all(&owner_root);
        std::fs::create_dir_all(owner_root.join("desired-state.json"))?;

        assert!(
            persist_owner_core_started(&owner, &ClashConfig::default())
                .await
                .is_err()
        );
        assert!(owner_root.join("desired-state.json").is_dir());
        std::fs::remove_dir_all(owner_root)?;
        Ok(())
    }
}
