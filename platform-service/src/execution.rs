//! Cooperative exclusion between Service supervision and local Sidecar execution.
#[cfg(unix)]
use anyhow::bail;
use anyhow::{Context as _, Result};
#[cfg(unix)]
use std::fs::OpenOptions;
#[cfg(any(unix, test))]
use std::path::Path;
#[cfg(unix)]
use std::{fs::File, path::PathBuf};

#[derive(Debug)]
pub struct CoreExecutionGuard {
    #[cfg(unix)]
    _file: File,
    #[cfg(windows)]
    _reservation: windows_lock::Reservation,
}

#[cfg(unix)]
impl Drop for CoreExecutionGuard {
    fn drop(&mut self) {
        // fork can briefly inherit this open file description before exec closes it.
        let _ = self._file.unlock();
    }
}

impl CoreExecutionGuard {
    pub fn acquire() -> Result<Self> {
        #[cfg(unix)]
        return Self::acquire_at(&coordination_path()?);
        #[cfg(windows)]
        {
            Ok(Self {
                _reservation: windows_lock::Reservation::try_acquire(windows_lock::NAME)?
                    .context("another Service or Sidecar owns core execution")?,
            })
        }
    }

    #[cfg(any(unix, test))]
    fn acquire_at(path: &Path) -> Result<Self> {
        #[cfg(windows)]
        {
            Ok(Self {
                _reservation: windows_lock::Reservation::try_acquire(&test_coordination_name(
                    path,
                ))?
                .context("core execution is reserved")?,
            })
        }
        #[cfg(unix)]
        {
            let file = open_coordination_file(path)?;
            match file.try_lock() {
                Ok(()) => Ok(Self { _file: file }),
                Err(std::fs::TryLockError::WouldBlock) => {
                    bail!("another Service or Sidecar owns core execution")
                }
                Err(std::fs::TryLockError::Error(error)) => {
                    Err(error).context("could not reserve core execution")
                }
            }
        }
    }

    /// Keep the reservation until the OS confirms exit, even if killing or event delivery fails.
    pub fn release_after_exit(
        self,
        pid: u32,
        mut terminated: tokio::sync::oneshot::Receiver<()>,
    ) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            let mut events_open = true;
            while !process_has_exited(pid) {
                tokio::select! {
                    result = &mut terminated, if events_open => {
                        if result.is_ok() { break; }
                        events_open = false;
                    }
                    _ = tokio::time::sleep(std::time::Duration::from_millis(50)) => {}
                }
            }
            drop(self);
        })
    }

    pub fn is_held() -> Result<bool> {
        #[cfg(windows)]
        {
            Ok(windows_lock::Reservation::try_acquire(windows_lock::NAME)?.is_none())
        }
        #[cfg(unix)]
        {
            let file = open_coordination_file(&coordination_path()?)?;
            match file.try_lock() {
                Ok(()) => {
                    file.unlock()?;
                    Ok(false)
                }
                Err(std::fs::TryLockError::WouldBlock) => Ok(true),
                Err(std::fs::TryLockError::Error(error)) => Err(error.into()),
            }
        }
    }
}

#[cfg(all(windows, test))]
fn test_coordination_name(path: &Path) -> String {
    use sha2::{Digest as _, Sha256};
    let digest: String = Sha256::digest(path.as_os_str().as_encoded_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    format!(r"Global\suki-clash-execution-test-{digest}")
}

#[cfg(unix)]
fn process_has_exited(pid: u32) -> bool {
    (unsafe { platform_lib::kill(pid as i32, 0) }) == -1
        && std::io::Error::last_os_error().raw_os_error() == Some(platform_lib::ESRCH)
}

#[cfg(windows)]
fn process_has_exited(pid: u32) -> bool {
    use std::os::windows::io::{AsRawHandle as _, FromRawHandle as _, OwnedHandle};
    use windows_sys::Win32::{
        Foundation::{ERROR_INVALID_PARAMETER, WAIT_OBJECT_0},
        System::Threading::{OpenProcess, SYNCHRONIZATION_SYNCHRONIZE, WaitForSingleObject},
    };
    let raw = unsafe { OpenProcess(SYNCHRONIZATION_SYNCHRONIZE, 0, pid) };
    if raw.is_null() {
        return std::io::Error::last_os_error().raw_os_error()
            == Some(ERROR_INVALID_PARAMETER as i32);
    }
    let handle = unsafe { OwnedHandle::from_raw_handle(raw) };
    (unsafe { WaitForSingleObject(handle.as_raw_handle(), 0) }) == WAIT_OBJECT_0
}

#[cfg(unix)]
fn coordination_path() -> Result<PathBuf> {
    #[cfg(unix)]
    let root = PathBuf::from("/tmp");
    #[cfg(windows)]
    let root = {
        use windows_sys::Win32::System::SystemInformation::GetWindowsDirectoryW;
        let mut buffer = vec![0u16; 32768];
        let size =
            unsafe { GetWindowsDirectoryW(buffer.as_mut_ptr(), buffer.len() as u32) } as usize;
        anyhow::ensure!(
            size > 0 && size < buffer.len(),
            "could not locate Windows coordination directory"
        );
        PathBuf::from(String::from_utf16(&buffer[..size])?).join("Temp")
    };
    // Both channels may keep idle helpers, but core execution is shared.
    #[cfg(feature = "test")]
    let name = "suki-clash-service.core-execution-test.lock";
    #[cfg(not(feature = "test"))]
    let name = "suki-clash-service.core-execution.lock";
    Ok(root.join(name))
}

#[cfg(unix)]
fn open_coordination_file(path: &Path) -> Result<File> {
    use std::os::unix::fs::{OpenOptionsExt as _, PermissionsExt as _};
    match OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .mode(0o444)
        .open(path)
    {
        Ok(file) => {
            file.set_permissions(std::fs::Permissions::from_mode(0o444))?;
            Ok(file)
        }
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            let file = OpenOptions::new()
                .read(true)
                .custom_flags(platform_lib::O_NOFOLLOW | platform_lib::O_NONBLOCK)
                .open(path)?;
            anyhow::ensure!(
                file.metadata()?.is_file(),
                "coordination entry is not a regular file"
            );
            Ok(file)
        }
        Err(error) => Err(error.into()),
    }
}

#[cfg(windows)]
#[path = "execution_windows_lock.rs"]
mod windows_lock;
