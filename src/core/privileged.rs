//! App-side CV service sessions. The lifecycle actor owns each proof; no UI or global owner state.
use super::launch::Launch;
use crate::storage::AppPaths;
use suki_service::{OwnerCredentials, OwnerSessionProof, ServiceStatusSnapshot};
pub(super) struct Session {
    credentials: OwnerCredentials,
    proof: OwnerSessionProof,
    pid: Option<u32>,
}
#[derive(Debug, thiserror::Error)]
pub(super) enum SessionFailure {
    #[error("后台服务内核归属已变化，未接管其他会话")]
    Displaced,
    #[error("服务内核未运行：{0}")]
    CoreStopped(String),
}
fn credentials(paths: &AppPaths) -> Result<OwnerCredentials, String> {
    super::service_identity::current_owner_credentials_for_root(paths.data())
        .map_err(crate::logging::error_message)
}
fn response<T>(reply: suki_service::Response<T>) -> Result<T, String> {
    suki_service::response_data(reply).map_err(|error| error.to_string())
}
pub(super) async fn require_ready() -> Result<(), String> {
    await_protocol(std::time::Duration::ZERO, suki_service::get_version)
        .await
        .map_err(|error| error.to_string())
}

/// Only an unavailable transport may settle with time. Rejections stop immediately.
async fn await_protocol<F, Fut>(budget: std::time::Duration, mut probe: F) -> anyhow::Result<()>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = anyhow::Result<suki_service::Response<suki_service::ProtocolInfo>>>,
{
    let deadline = tokio::time::Instant::now() + budget;
    let mut previous = None;
    loop {
        let result = if budget.is_zero() {
            probe().await
        } else {
            match tokio::time::timeout_at(deadline, probe()).await {
                Ok(result) => result,
                Err(_) => {
                    return Err(previous.unwrap_or_else(|| {
                        suki_service::RequestError::Unreachable("等待服务响应超时".into()).into()
                    }));
                }
            }
        };
        match result {
            Ok(reply) => return suki_service::validate_protocol_response(reply),
            Err(error) => {
                if !suki_service::is_unreachable(&error) {
                    return Err(error);
                }
                let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
                if remaining.is_zero() {
                    return Err(error);
                }
                previous = Some(error);
                tokio::time::sleep(remaining.min(std::time::Duration::from_millis(200))).await;
            }
        }
    }
}

#[cfg(not(test))]
pub(super) async fn probe_startup(tun_requested: bool) -> State {
    #[cfg(windows)]
    let budget = if tun_requested {
        match crate::platform_access::elevated() {
            Ok(false) => std::time::Duration::from_secs(30),
            Ok(true) => std::time::Duration::ZERO,
            Err(error) => return State::Rejected(error),
        }
    } else {
        std::time::Duration::ZERO
    };
    #[cfg(not(windows))]
    let budget = {
        let _ = tun_requested;
        std::time::Duration::ZERO
    };
    match await_protocol(budget, suki_service::get_version).await {
        Ok(()) => State::Ready,
        Err(error) => State::from_error(error),
    }
}
/// Existing elevated privileges can run TUN directly; never request elevation during a toggle.
pub(super) async fn tun_service_required() -> Result<bool, String> {
    match await_protocol(std::time::Duration::ZERO, suki_service::get_version).await {
        Ok(()) => Ok(true),
        Err(error) if suki_service::is_unreachable(&error) => {
            elevated_tun_policy(crate::platform_access::elevated()?, error.to_string())
        }
        Err(error) => Err(error.to_string()),
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
    #[tokio::test]
    async fn temporary_service_transport_waits_but_readable_rejections_never_retry() {
        use std::{cell::Cell, time::Duration};
        let attempts = Cell::new(0);
        super::await_protocol(Duration::from_secs(1), || {
            let attempt = attempts.get();
            attempts.set(attempt + 1);
            std::future::ready(if attempt < 2 {
                Err(suki_service::RequestError::Unreachable("pipe not ready".into()).into())
            } else {
                Ok(suki_service::Response {
                    code: 0,
                    message: String::new(),
                    data: Some(suki_service::ProtocolInfo::current()),
                })
            })
        })
        .await
        .unwrap();
        assert_eq!(attempts.get(), 3);

        let mut wrong_protocol = suki_service::ProtocolInfo::current();
        wrong_protocol.protocol.revision += 1;
        for result in [
            Ok(suki_service::Response {
                code: 403,
                message: "session denied".into(),
                data: None,
            }),
            Ok(suki_service::Response {
                code: 0,
                message: String::new(),
                data: Some(wrong_protocol),
            }),
            Err(anyhow::Error::new(
                suki_service::RequestError::AccessDenied("permission denied".into()),
            )),
            Err(anyhow::Error::new(
                suki_service::RequestError::InvalidResponse("bad JSON".into()),
            )),
        ] {
            let attempts = Cell::new(0);
            let mut result = Some(result);
            let error = super::await_protocol(Duration::from_secs(1), || {
                attempts.set(attempts.get() + 1);
                std::future::ready(
                    result
                        .take()
                        .expect("a terminal response must never be retried"),
                )
            })
            .await
            .unwrap_err();
            assert_eq!(attempts.get(), 1);
            assert!(!suki_service::is_unreachable(&error));
            assert!(matches!(
                super::State::from_error(error),
                super::State::Rejected(_)
            ));
        }
    }

    #[tokio::test]
    async fn service_wait_deadline_retains_transport_reason_and_bounds_a_silent_probe() {
        use std::time::Duration;
        let error = super::await_protocol(Duration::from_millis(100), || {
            std::future::ready(Err(suki_service::RequestError::Unreachable(
                "pipe not found".into(),
            )
            .into()))
        })
        .await
        .unwrap_err();
        assert!(error.to_string().contains("pipe not found"));
        assert!(matches!(
            super::State::from_error(error),
            super::State::Unreachable(_)
        ));
        let started = tokio::time::Instant::now();
        let error = super::await_protocol(Duration::from_millis(100), std::future::pending)
            .await
            .unwrap_err();
        assert!(suki_service::is_unreachable(&error));
        assert!(started.elapsed() < Duration::from_secs(1));
    }

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
            Err(error) if suki_service::is_unreachable(&error) => {
                log::debug!("后台服务未确认启动应答，将在同一会话中确认：{error:#}");
                suki_service::start_clash(&credentials, &request)
                    .await
                    .map_err(|error| format!("服务启动请求未确认，未启动其他内核：{error}"))?
            }
            Err(error) => return Err(error.to_string()),
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
    pub(super) async fn status(&mut self) -> anyhow::Result<ServiceStatusSnapshot> {
        let reply = suki_service::get_status(&self.credentials).await?;
        if reply.code == suki_service::ServiceErrorCode::NotActive as u16
            || reply.code == suki_service::ServiceErrorCode::StaleOwnerSession as u16
        {
            return Err(SessionFailure::Displaced.into());
        }
        let status = suki_service::response_data(reply)?;
        if !status.is_active || status.active_generation != Some(self.proof.generation) {
            return Err(SessionFailure::Displaced.into());
        }
        if !status.desired_core_should_be_running
            || matches!(
                status.service_state,
                suki_service::ServiceLifecycleState::Fatal
            )
        {
            return Err(SessionFailure::CoreStopped(
                status
                    .last_core_exit_reason
                    .clone()
                    .unwrap_or_else(|| format!("{:?}", status.service_state)),
            )
            .into());
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
    pub(super) async fn sync_geo(&self, paths: &AppPaths) -> Result<usize, String> {
        super::geo_sync::sync(
            &paths.directory(crate::storage::DirectoryKind::Core),
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
        )
        .await
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
    getrandom::fill(&mut bytes).map_err(crate::logging::error_message)?;
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}
#[derive(Clone)]
pub(crate) enum Action {
    Install,
    Repair,
    Uninstall,
}
pub(super) async fn manage(
    paths: &AppPaths,
) -> Result<Vec<suki_service::management::InstallationWarning>, String> {
    install(paths, false).await
}
pub(super) async fn uninstall() -> Result<(), String> {
    tokio::task::spawn_blocking(|| {
        let service = std::env::current_exe()
            .map_err(crate::logging::error_message)?
            .with_file_name(format!(
                "suki-clash-service{}",
                std::env::consts::EXE_SUFFIX
            ));
        if !service.is_file() {
            return Err("后台服务程序不存在，请使用完整应用包".into());
        }
        suki_service::management::uninstall(&service).map_err(crate::logging::error_message)
    })
    .await
    .map_err(|error| crate::logging::diagnostic_error("服务卸载任务异常退出", error))?
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
        let service = std::env::current_exe()
            .map_err(crate::logging::error_message)?
            .with_file_name(format!(
                "suki-clash-service{}",
                std::env::consts::EXE_SUFFIX
            ));
        if !service.is_file() {
            return Err("后台服务程序不存在，请使用完整应用包".into());
        }
        #[cfg(unix)]
        let gid = Some(unsafe { libc::getegid() });
        #[cfg(windows)]
        let gid = None;
        suki_service::management::install(
            &service,
            &[suki_service::management::CoreSource {
                path: core,
                name: crate::bootstrap::core_name().into(),
            }],
            core_only,
            gid,
            "安装 Suki Clash 后台服务",
        )
        .map_err(crate::logging::error_message)
    })
    .await
    .map_err(|error| crate::logging::diagnostic_error("服务安装任务异常退出", error))??;
    await_protocol(
        std::time::Duration::from_secs(30),
        suki_service::get_version,
    )
    .await
    .map_err(|error| format!("安装操作已完成；后台服务状态未确认：{error}"))?;
    Ok(outcome.warnings)
}

#[cfg(not(test))]
pub(super) async fn ready_for_start(paths: &AppPaths) -> Result<bool, String> {
    match suki_service::get_version().await {
        Ok(reply) => {
            suki_service::validate_protocol_response(reply).map_err(|error| error.to_string())?
        }
        Err(error) if suki_service::is_unreachable(&error) => return Ok(false),
        Err(error) => return Err(error.to_string()),
    }
    response(
        suki_service::get_status(&credentials(paths)?)
            .await
            .map_err(|error| error.to_string())?,
    )?;
    Ok(true)
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) enum State {
    #[default]
    Checking,
    Ready,
    Unreachable(String),
    Rejected(String),
}
impl State {
    fn from_error(error: anyhow::Error) -> Self {
        if suki_service::is_unreachable(&error) {
            Self::Unreachable(error.to_string())
        } else {
            Self::Rejected(error.to_string())
        }
    }
    pub(crate) fn reason(&self) -> Option<&str> {
        match self {
            Self::Unreachable(reason) | Self::Rejected(reason) => Some(reason),
            Self::Checking | Self::Ready => None,
        }
    }
    pub(crate) fn label(&self) -> &str {
        match self {
            Self::Checking => "读取中",
            Self::Ready => "就绪",
            Self::Unreachable(_) => "尚未连接",
            Self::Rejected(_) => "服务异常",
        }
    }
}
pub(super) async fn probe() -> State {
    match await_protocol(std::time::Duration::ZERO, suki_service::get_version).await {
        Ok(()) => State::Ready,
        Err(error) => State::from_error(error),
    }
}
