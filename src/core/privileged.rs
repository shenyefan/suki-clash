//! App-side CV service sessions. The lifecycle actor owns each proof; no UI or global owner state.
use super::launch::Launch;
use crate::storage::AppPaths;
use suki_service::{OwnerCredentials, OwnerSessionProof, ProtocolVersion, ServiceStatusSnapshot};
pub(super) struct Session {
    credentials: OwnerCredentials,
    proof: OwnerSessionProof,
    pid: Option<u32>,
}
fn credentials(paths: &AppPaths) -> Result<OwnerCredentials, String> {
    super::service_identity::current_owner_credentials_for_root(paths.data())
        .map_err(|error| error.to_string())
}
fn response<T>(response: suki_service::Response<T>) -> Result<T, String> {
    if response.code != 0 {
        return Err(format!(
            "后台服务拒绝操作（{}）：{}",
            response.code, response.message
        ));
    }
    response.data.ok_or_else(|| "后台服务未返回结果".into())
}
pub(super) async fn require_ready() -> Result<(), String> {
    let version = suki_service::get_version()
        .await
        .map_err(|_| "后台服务未就绪，请在虚拟网卡页安装或修复服务")?;
    let version = response(version)?;
    if version.protocol != ProtocolVersion::current() {
        return Err("后台服务协议不一致，请修复服务".into());
    }
    Ok(())
}
/// Existing elevated privileges can run TUN directly; never request elevation during a toggle.
pub(super) async fn tun_service_required() -> Result<bool, String> {
    match require_ready().await {
        Ok(()) => Ok(true),
        Err(error) => elevated_tun_policy(crate::platform_access::elevated()?, error),
    }
}
fn elevated_tun_policy(elevated: bool, service_error: String) -> Result<bool, String> {
    if elevated {
        Ok(false)
    } else {
        Err(service_error)
    }
}
#[cfg(test)]
mod policy_tests {
    #[test]
    fn generated_session_tokens_meet_the_current_service_contract() {
        let first = super::session_token().unwrap();
        let second = super::session_token().unwrap();
        assert_eq!(first.len(), suki_service::SESSION_TOKEN_HEX_LEN);
        assert!(
            first
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        );
        assert_ne!(first, second);
    }

    #[test]
    fn existing_privilege_allows_direct_tun_but_normal_users_keep_service_error() {
        assert_eq!(
            super::elevated_tun_policy(true, "unavailable".into()),
            Ok(false)
        );
        assert_eq!(
            super::elevated_tun_policy(false, "permission denied".into()),
            Err("permission denied".into())
        );
    }
}
impl Session {
    pub(super) async fn start(paths: &AppPaths, launch: &mut Launch) -> Result<Self, String> {
        require_ready().await?;
        let credentials = credentials(paths)?;
        let token = session_token()?;
        let request = suki_service::StartClashRequest {
            runtime: launch.service_bundle().await?,
            proposed_session_token: token.clone(),
            macos_proxy: None,
        };
        let reply = match suki_service::start_clash(&credentials, &request).await {
            Ok(reply) => reply,
            // The service deduplicates this exact session token. Retry once if the acknowledgement was lost.
            Err(_) => suki_service::start_clash(&credentials, &request)
                .await
                .map_err(|error| format!("服务启动请求未确认；未启动其他内核：{error}"))?,
        };
        let started = response(reply)?;
        let proof = OwnerSessionProof {
            generation: started.session.generation,
            token,
        };
        launch.service_endpoint(&credentials.identity);
        Ok(Self {
            credentials,
            proof,
            pid: None,
        })
    }
    pub(super) fn owner_session(&self) -> (OwnerCredentials, OwnerSessionProof) {
        (self.credentials.clone(), self.proof.clone())
    }
    pub(super) fn id(&self) -> Option<u32> {
        self.pid
    }
    pub(super) async fn status(&mut self) -> Result<ServiceStatusSnapshot, String> {
        let status = response(
            suki_service::get_status(&self.credentials)
                .await
                .map_err(|_| "后台服务连接中断，未启动第二个内核")?,
        )?;
        if !status.is_active || status.active_generation != Some(self.proof.generation) {
            return Err("后台服务内核归属已变化，未接管其他会话".into());
        }
        if status.core_pid.is_none() {
            return Err(format!("服务内核未运行：{:?}", status.service_state));
        }
        self.pid = status.core_pid;
        Ok(status)
    }
    pub(super) async fn stop(&self) -> Result<(), String> {
        let result = suki_service::stop_clash(&self.credentials, &self.proof).await;
        // A lost acknowledgement is resolved by ownership readback, never by launching a replacement.
        let status = response(
            suki_service::get_status(&self.credentials)
                .await
                .map_err(|error| format!("服务停止后状态未知：{error}"))?,
        )?;
        if status.is_active && status.active_generation == Some(self.proof.generation) {
            return Err(match result {
                Ok(reply) => format!("服务仍持有本应用内核（{}）：{}", reply.code, reply.message),
                Err(error) => format!("服务停止请求未确认：{error}"),
            });
        }
        Ok(())
    }
    pub(super) async fn sync_providers(
        &self,
        launch: &Launch,
        paths: &AppPaths,
    ) -> Result<usize, String> {
        super::provider_sync::sync(
            &launch.client,
            launch,
            paths,
            |destination, offset| async move {
                response(
                    suki_service::read_runtime_file(
                        &self.credentials,
                        &self.proof,
                        &suki_service::RuntimeFileRequest {
                            destination,
                            offset,
                        },
                    )
                    .await
                    .map_err(|error| error.to_string())?,
                )
            },
            || async {
                let status = response(
                    suki_service::get_status(&self.credentials)
                        .await
                        .map_err(|error| error.to_string())?,
                )?;
                if !status.is_active || status.active_generation != Some(self.proof.generation) {
                    return Err("Provider 同步期间后台服务归属已变化".into());
                }
                Ok(())
            },
            false,
        )
        .await
    }
    pub(super) async fn stage(
        &self,
        launch: &Launch,
    ) -> Result<suki_service::StageRuntimeOutcome, String> {
        let outcome = response(
            suki_service::stage_runtime(
                &self.credentials,
                &self.proof,
                &launch.service_bundle().await?,
            )
            .await
            .map_err(|error| error.to_string())?,
        )?;
        Ok(outcome)
    }
}

fn session_token() -> Result<String, String> {
    let mut bytes = [0u8; suki_service::SESSION_TOKEN_HEX_LEN / 2];
    getrandom::fill(&mut bytes).map_err(|error| error.to_string())?;
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}
#[derive(Clone)]
pub(crate) enum Action {
    Install,
    Repair,
    Stop,
    Uninstall,
    Sidecar,
}
pub(super) async fn manage(
    paths: &AppPaths,
) -> Result<Vec<suki_service::management::InstallationWarning>, String> {
    install(paths, false).await
}
pub(super) async fn uninstall() -> Result<(), String> {
    tokio::task::spawn_blocking(|| {
        let tool = std::env::current_exe()
            .map_err(|error| error.to_string())?
            .with_file_name(format!(
                "suki-clash-service-uninstall{}",
                std::env::consts::EXE_SUFFIX
            ));
        if !tool.is_file() {
            return Err("服务卸载工具不存在".into());
        }
        suki_service::management::uninstall(&tool).map_err(|error| error.to_string())
    })
    .await
    .map_err(|_| "服务卸载任务异常退出")?
}
pub(super) async fn publish_core(
    paths: &AppPaths,
) -> Result<Vec<suki_service::management::InstallationWarning>, String> {
    install(paths, true).await
}
async fn install(
    paths: &AppPaths,
    core_only: bool,
) -> Result<Vec<suki_service::management::InstallationWarning>, String> {
    let core = paths
        .directory(crate::storage::DirectoryKind::Core)
        .join(crate::bootstrap::core_name());
    let outcome = tokio::task::spawn_blocking(move || {
        let installer = std::env::current_exe()
            .map_err(|error| error.to_string())?
            .with_file_name(format!(
                "suki-clash-service-install{}",
                std::env::consts::EXE_SUFFIX
            ));
        if !installer.is_file() {
            return Err("服务安装工具不存在，请使用包含后台服务的完整应用包".into());
        }
        #[cfg(unix)]
        let gid = Some(unsafe { libc::getegid() });
        #[cfg(windows)]
        let gid = None;
        suki_service::management::install(
            &installer,
            &[suki_service::management::CoreSource {
                path: core,
                name: crate::bootstrap::core_name().into(),
            }],
            core_only,
            gid,
            "安装 Suki Clash 后台服务",
        )
        .map_err(|error| error.to_string())
    })
    .await
    .map_err(|_| "服务安装任务异常退出")??;
    require_ready()
        .await
        .map_err(|error| format!("安装操作已完成；后台服务状态未确认：{error}"))?;
    Ok(outcome.warnings)
}

#[cfg(not(test))]
pub(super) async fn owner_active(paths: &AppPaths) -> Result<bool, String> {
    #[cfg(unix)]
    if !suki_service::is_ipc_path_exists() {
        return Ok(false);
    }
    #[cfg(windows)]
    if suki_service::get_version().await.is_err() {
        return Ok(false);
    }
    require_ready().await?;
    Ok(response(
        suki_service::get_status(&credentials(paths)?)
            .await
            .map_err(|error| error.to_string())?,
    )?
    .is_active)
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) enum State {
    #[default]
    Checking,
    Ready,
    Unavailable(String),
}
impl State {
    pub(crate) fn label(&self) -> &str {
        match self {
            Self::Checking => "读取中",
            Self::Ready => "就绪",
            Self::Unavailable(_) => "未连接",
        }
    }
}
pub(super) async fn probe() -> State {
    match require_ready().await {
        Ok(()) => State::Ready,
        Err(error) => State::Unavailable(error),
    }
}
