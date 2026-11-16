use crate::core::desired::{
    clear_active_owner, load_active_owner, persist_owner_core_stopped_by_key,
};
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

    if let Some(record) = read_core_runtime_record().await? {
        let current_identity = process_identity(record.pid)?;
        let socket_reachable = is_core_socket_reachable(&record.ipc_path).await;

        if current_identity.as_ref() == Some(&record.identity) {
            warn!("启动时发现已验证的旧内核进程 {}，停止旧会话", record.pid);
            terminate_process_if_identity(record.pid, &record.identity, CORE_TERM_GRACE).await?;
            cleanup_core_socket(&record.ipc_path).await;
        } else {
            if let Some(current_identity) = current_identity {
                warn!(
                    "运行记录中的 PID {} 已属于其他进程（{:?}），不予终止",
                    record.pid, current_identity
                );
            }
            if !socket_reachable {
                info!("清理已退出进程遗留的内核套接字 {}", record.ipc_path);
                cleanup_core_socket(&record.ipc_path).await;
            } else {
                warn!(
                    "内核进程 {} 已退出但套接字 {} 仍可连接，保留不清理",
                    record.pid, record.ipc_path
                );
            }
        }
        remove_core_runtime_record().await;
    }

    // Persisted running intent belongs to the previous daemon session, not to OS autostart.
    // Expire it after confirming the old process is gone so status and installation probes
    // cannot report an idle helper as recovering or busy, or accept the previous session proof.
    if let Some(owner) = load_active_owner().await? {
        persist_owner_core_stopped_by_key(&owner.owner_key).await?;
        clear_active_owner().await?;
    }
    STARTUP_RECONCILED.store(true, Ordering::Release);
    Ok(())
}

#[cfg(all(test, feature = "test"))]
mod tests {
    use super::*;
    use crate::core::auth::AuthenticatedOwner;
    use crate::core::desired::{
        commit_active_owner_session, load_owner_desired_state, persist_owner_core_started,
    };
    use crate::core::paths::service_paths;
    use crate::core::runtime::{CoreRuntimeRecord, write_core_runtime_record};
    use crate::core::status::service_status_snapshot;
    use crate::{OwnerIdentity, ServiceLifecycleState};
    use serial_test::serial;

    fn owner() -> AuthenticatedOwner {
        AuthenticatedOwner {
            key: "90019".into(),
            identity: OwnerIdentity::Unix {
                uid: 90019,
                gid: 20,
            },
            app_data_root: std::env::temp_dir(),
        }
    }

    #[tokio::test]
    #[serial]
    async fn daemon_start_expires_running_intent_without_starting_a_core() -> Result<()> {
        let owner = owner();
        for stale_runtime in [false, true] {
            persist_owner_core_started(&owner).await?;
            let previous = commit_active_owner_session(&owner, &"11".repeat(32)).await?;
            remove_core_runtime_record().await;
            if stale_runtime {
                // The PID is live but its recorded identity differs: never terminate it.
                let mut identity = process_identity(std::process::id())?.unwrap();
                identity.started_at = identity.started_at.wrapping_add(1);
                write_core_runtime_record(&CoreRuntimeRecord {
                    pid: std::process::id(),
                    ipc_path: service_paths()?
                        .for_owner_key(&owner.key)
                        .root()
                        .join("absent.sock")
                        .to_string_lossy()
                        .into_owned(),
                    identity,
                })
                .await?;
            }

            reconcile_service_startup().await?;
            ensure_startup_reconciled()?;
            assert!(load_active_owner().await?.is_none());
            assert!(
                !load_owner_desired_state(&owner.key)
                    .await?
                    .core_should_be_running
            );
            assert!(read_core_runtime_record().await?.is_none());
            let status = service_status_snapshot(&owner).await?;
            assert!(!status.is_active);
            assert_eq!(status.active_generation, None);
            assert_eq!(status.core_pid, None);
            assert_ne!(status.service_state, ServiceLifecycleState::RecoveringCore);
            assert!(
                !crate::core::installation::inspect(&[], false)
                    .await?
                    .core_busy
            );
            assert!(process_identity(std::process::id())?.is_some());

            // A later app request receives a fresh session; old proofs cannot regain ownership.
            let next = commit_active_owner_session(&owner, &"22".repeat(32)).await?;
            assert!(next.generation > previous.generation);
            clear_active_owner().await?;
        }
        Ok(())
    }

    #[tokio::test]
    #[serial]
    async fn corrupt_startup_state_blocks_start_without_overwriting_the_file() -> Result<()> {
        let owner = owner();
        persist_owner_core_started(&owner).await?;
        commit_active_owner_session(&owner, &"33".repeat(32)).await?;
        remove_core_runtime_record().await;
        let path = service_paths()?
            .for_owner_key(&owner.key)
            .desired_state_path();
        let original = std::fs::read(&path)?;
        std::fs::write(&path, b"{broken")?;

        let result = reconcile_service_startup().await;
        assert!(result.is_err());
        assert!(ensure_startup_reconciled().is_err());
        assert_eq!(std::fs::read(&path)?, b"{broken");
        assert!(load_active_owner().await?.is_some());

        std::fs::write(&path, original)?;
        reconcile_service_startup().await?;
        ensure_startup_reconciled()?;
        assert!(load_active_owner().await?.is_none());
        Ok(())
    }

    #[tokio::test]
    #[serial]
    async fn daemon_start_stops_verified_previous_process_without_replacing_it() -> Result<()> {
        #[cfg(windows)]
        let mut command = {
            let mut command = tokio::process::Command::new("powershell.exe");
            command.args(["-NoProfile", "-Command", "Start-Sleep -Seconds 60"]);
            command.creation_flags(0x08000000);
            command
        };
        #[cfg(unix)]
        let mut command = {
            let mut command = tokio::process::Command::new("sleep");
            command.arg("60");
            command
        };
        let mut child = command.kill_on_drop(true).spawn()?;
        let pid = child.id().unwrap();
        let identity = process_identity(pid)?.unwrap();
        let owner = owner();
        persist_owner_core_started(&owner).await?;
        commit_active_owner_session(&owner, &"44".repeat(32)).await?;
        write_core_runtime_record(&CoreRuntimeRecord {
            pid,
            ipc_path: service_paths()?
                .for_owner_key(&owner.key)
                .root()
                .join("absent.sock")
                .to_string_lossy()
                .into_owned(),
            identity,
        })
        .await?;

        reconcile_service_startup().await?;

        tokio::time::timeout(std::time::Duration::from_secs(5), child.wait()).await??;
        assert!(load_active_owner().await?.is_none());
        assert!(
            !load_owner_desired_state(&owner.key)
                .await?
                .core_should_be_running
        );
        assert!(read_core_runtime_record().await?.is_none());
        assert_eq!(service_status_snapshot(&owner).await?.core_pid, None);
        Ok(())
    }
}
