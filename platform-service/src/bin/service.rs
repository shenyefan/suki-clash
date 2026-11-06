//! Cross-platform IPC daemon, run standalone or as a Windows service.

#[path = "../maintenance/mod.rs"]
mod maintenance;

use anyhow::Result;
use suki_clash_service::{
    acquire_service_owner, flush_service_log, init_service_logging, reconcile_service_startup,
    restore_desired_state, run_ipc_supervisor_until_shutdown,
};
use tracing::{info, warn};

#[cfg(windows)]
use {
    platform_lib::{
        define_windows_service,
        service::{
            ServiceControl, ServiceControlAccept, ServiceExitCode, ServiceState, ServiceStatus,
            ServiceType,
        },
        service_control_handler::{self, ServiceControlHandlerResult},
        service_dispatcher,
    },
    std::ffi::OsString,
    std::time::Duration,
};

fn main() -> Result<()> {
    if maintenance::run_if_requested()? {
        return Ok(());
    }
    run_daemon()
}

#[cfg(not(windows))]
#[tokio::main(flavor = "current_thread")]
async fn run_daemon() -> Result<()> {
    set_secure_process_umask();
    init_service_logging();
    let result = run_standalone().await;
    if let Err(error) = &result {
        warn!("服务异常退出（{error:#}）");
    }
    flush_service_log();
    result
}

#[cfg(unix)]
fn set_secure_process_umask() {
    unsafe {
        platform_lib::umask(0o077);
    }
}

/// Runs as a Windows service when possible, otherwise standalone.
#[cfg(windows)]
fn run_daemon() -> Result<()> {
    init_service_logging();
    if service_dispatcher::start(suki_clash_service::WINDOWS_SERVICE_NAME, ffi_service_main)
        .is_err()
    {
        info!("未作为系统服务运行，以独立模式启动");
        let result = tokio::runtime::Runtime::new()
            .map_err(anyhow::Error::from)
            .and_then(|rt| rt.block_on(run_standalone()));
        if let Err(error) = &result {
            warn!("服务异常退出（{error:#}）");
        }
        flush_service_log();
        result?;
    }
    Ok(())
}

#[cfg(windows)]
define_windows_service!(ffi_service_main, my_service_main);

#[cfg(windows)]
fn my_service_main(_args: Vec<OsString>) {
    if let Err(e) = run_service() {
        info!("服务运行失败（{}）", e);
    }
    flush_service_log();
}

#[cfg(windows)]
fn run_service() -> platform_lib::Result<()> {
    let (shutdown_tx, mut shutdown_rx) = tokio::sync::mpsc::channel::<()>(1);

    let event_handler = move |control_event| -> ServiceControlHandlerResult {
        match control_event {
            ServiceControl::Stop | ServiceControl::Shutdown => {
                let _ = shutdown_tx.try_send(());
                ServiceControlHandlerResult::NoError
            }
            ServiceControl::Interrogate => ServiceControlHandlerResult::NoError,
            _ => ServiceControlHandlerResult::NotImplemented,
        }
    };

    let status_handle =
        service_control_handler::register(suki_clash_service::WINDOWS_SERVICE_NAME, event_handler)?;

    status_handle.set_service_status(ServiceStatus {
        service_type: ServiceType::OWN_PROCESS,
        current_state: ServiceState::Running,
        controls_accepted: ServiceControlAccept::STOP | ServiceControlAccept::SHUTDOWN,
        exit_code: ServiceExitCode::Win32(0),
        checkpoint: 0,
        wait_hint: Duration::default(),
        process_id: None,
    })?;

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let fatal = rt.block_on(async {
        let owner_guard = match acquire_service_owner().await {
            Ok(Some(owner_guard)) => owner_guard,
            Ok(None) => return false,
            Err(error) => {
                tracing::warn!("获取服务所有权锁失败（{}）", error);
                return true;
            }
        };

        match reconcile_service_startup().await {
            Ok(()) => {
                if let Err(error) = restore_desired_state().await {
                    tracing::warn!(
                        "期望状态恢复失败，保留本地通道供界面恢复（{:#}）",
                        error
                    );
                }
            }
            Err(error) => tracing::warn!(
                "服务启动对账失败，内核启动保持阻止，本地通道仍可用（{}）",
                error
            ),
        }

        let result = run_ipc_supervisor_until_shutdown(async {
            let _ = shutdown_rx.recv().await;
        })
        .await;
        if let Err(error) = result {
            tracing::warn!("本地通道监督失败（{}）", error);
            drop(owner_guard);
            return true;
        }

        drop(owner_guard);
        false
    });
    // The SCM may end the process as soon as it sees `Stopped`.
    flush_service_log();

    status_handle.set_service_status(ServiceStatus {
        service_type: ServiceType::OWN_PROCESS,
        current_state: ServiceState::Stopped,
        controls_accepted: ServiceControlAccept::empty(),
        exit_code: ServiceExitCode::Win32(if fatal { 1 } else { 0 }),
        checkpoint: 0,
        wait_hint: Duration::default(),
        process_id: None,
    })?;

    if fatal {
        std::process::exit(1);
    }

    Ok(())
}

async fn run_standalone() -> Result<()> {
    let pid = std::process::id();
    info!("Suki Clash 服务独立模式");
    info!("当前进程 PID {}", pid);

    let Some(_owner_guard) = acquire_service_owner().await? else {
        return Ok(());
    };

    // Startup recovery is best-effort so stale desired state cannot prevent IPC self-healing.
    match reconcile_service_startup().await {
        Ok(()) => {
            if let Err(error) = restore_desired_state().await {
                warn!(
                    "启动时恢复期望内核状态失败，不自动启动内核，保留本地通道供界面重连恢复（{error:#}）"
                );
            }
        }
        Err(error) => {
            warn!(
                "服务启动对账失败，内核启动保持阻止，本地通道仍可用（{error:#}）"
            )
        }
    }

    let result = run_ipc_supervisor_until_shutdown(shutdown_signal()).await;
    #[cfg(target_os = "macos")]
    if let Err(error) = &result
        && error.is::<suki_clash_service::IpcRecoveryBudgetExhausted>()
    {
        // An intentional exhausted supervisor must not be relaunched forever by launchd.
        warn!("服务恢复次数已耗尽，需要手动修复（{error:#}）");
        return Ok(());
    }
    result?;

    info!("服务已停止");
    Ok(())
}

async fn shutdown_signal() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};
        let mut sigint = signal(SignalKind::interrupt()).expect("Failed to install SIGINT handler");
        let mut sigterm =
            signal(SignalKind::terminate()).expect("Failed to install SIGTERM handler");

        tokio::select! {
            _ = sigint.recv() => info!("收到 SIGINT（Ctrl+C）"),
            _ = sigterm.recv() => info!("收到 SIGTERM"),
        }
    }

    #[cfg(windows)]
    {
        use tokio::signal::windows::{ctrl_break, ctrl_c, ctrl_close, ctrl_logoff, ctrl_shutdown};

        let mut ctrl_c = ctrl_c().expect("Failed to install Ctrl+C handler");
        let mut ctrl_break = ctrl_break().expect("Failed to install Ctrl+Break handler");
        let mut ctrl_close = ctrl_close().expect("Failed to install Ctrl+Close handler");
        let mut ctrl_logoff = ctrl_logoff().expect("Failed to install Ctrl+Logoff handler");
        let mut ctrl_shutdown = ctrl_shutdown().expect("Failed to install Ctrl+Shutdown handler");

        tokio::select! {
            _ = ctrl_c.recv() => info!("收到 Ctrl+C"),
            _ = ctrl_break.recv() => info!("收到 Ctrl+Break"),
            _ = ctrl_close.recv() => info!("收到控制台关闭信号"),
            _ = ctrl_logoff.recv() => info!("收到用户注销信号"),
            _ = ctrl_shutdown.recv() => info!("收到系统关机信号"),
        }
    }
}
