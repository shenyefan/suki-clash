use crate::core::manager::CORE_TERM_GRACE;
use crate::core::process::{process_identity, terminate_process_if_identity};
use crate::core::runtime::{
    cleanup_core_socket, is_core_socket_reachable, read_core_runtime_record,
    remove_core_runtime_record,
};
use anyhow::Result;
use std::sync::atomic::{AtomicBool, Ordering};
use tracing::{info, warn};

static STARTUP_RECONCILED: AtomicBool = AtomicBool::new(cfg!(feature = "test"));

pub(super) fn ensure_startup_reconciled() -> Result<()> {
    anyhow::ensure!(
        STARTUP_RECONCILED.load(Ordering::Acquire),
        "core startup reconciliation has not completed"
    );
    Ok(())
}

pub async fn reconcile_service_startup() -> Result<()> {
    STARTUP_RECONCILED.store(false, Ordering::Release);
    info!("正在执行服务启动对账");

    let Some(record) = read_core_runtime_record().await? else {
        STARTUP_RECONCILED.store(true, Ordering::Release);
        return Ok(());
    };

    let current_identity = process_identity(record.pid)?;
    let socket_reachable = is_core_socket_reachable(&record.ipc_path).await;

    if current_identity.as_ref() == Some(&record.identity) {
        warn!(
            "启动时发现已验证的旧内核进程 {}，先停止再恢复看护",
            record.pid
        );
        terminate_process_if_identity(record.pid, &record.identity, CORE_TERM_GRACE).await?;
        cleanup_core_socket(&record.ipc_path).await;
        remove_core_runtime_record().await;
        STARTUP_RECONCILED.store(true, Ordering::Release);
        return Ok(());
    }

    if let Some(current_identity) = current_identity {
        warn!(
            "运行记录中的 PID {} 已属于其他进程（{:?}），不予终止",
            record.pid, current_identity
        );
    }
    if !socket_reachable {
        info!(
            "清理已退出进程遗留的内核套接字 {}",
            record.ipc_path
        );
        cleanup_core_socket(&record.ipc_path).await;
    } else {
        warn!(
            "内核进程 {} 已退出但套接字 {} 仍可连接，保留不清理",
            record.pid, record.ipc_path
        );
    }

    remove_core_runtime_record().await;
    STARTUP_RECONCILED.store(true, Ordering::Release);
    Ok(())
}
