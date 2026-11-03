use anyhow::{Context as _, Result, bail};
use std::{
    os::windows::io::{AsRawHandle as _, FromRawHandle as _, OwnedHandle},
    sync::mpsc,
    thread,
};
use windows_sys::Win32::{
    Foundation::{LocalFree, WAIT_ABANDONED, WAIT_OBJECT_0, WAIT_TIMEOUT},
    Security::{
        Authorization::{ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1},
        SECURITY_ATTRIBUTES,
    },
    System::Threading::{CreateMutexExW, ReleaseMutex, WaitForSingleObject},
};

#[cfg(not(feature = "test"))]
pub(super) const NAME: &str = r"Global\suki-clash-service.core-execution";
#[cfg(feature = "test")]
pub(super) const NAME: &str = r"Global\suki-clash-service.core-execution-test";

#[derive(Debug)]
pub(super) struct Reservation {
    release: Option<mpsc::Sender<()>>,
    worker: Option<thread::JoinHandle<()>>,
}
impl Reservation {
    pub(super) fn try_acquire(name: &str) -> Result<Option<Self>> {
        let name = name.to_owned();
        let (ready_tx, ready_rx) = mpsc::sync_channel(0);
        let (release_tx, release_rx) = mpsc::channel();
        let worker = thread::Builder::new()
            .name("core-execution-lock".into())
            .spawn(move || match MutexOwner::try_acquire(&name) {
                Ok(Some(owner)) => {
                    if ready_tx.send(Ok(true)).is_ok() {
                        let _ = release_rx.recv();
                    }
                    drop(owner);
                }
                Ok(None) => {
                    let _ = ready_tx.send(Ok(false));
                }
                Err(error) => {
                    let _ = ready_tx.send(Err(error));
                }
            })
            .context("could not start core execution lock owner")?;
        match ready_rx
            .recv()
            .context("core execution lock owner exited")?
        {
            Ok(true) => Ok(Some(Self {
                release: Some(release_tx),
                worker: Some(worker),
            })),
            result => {
                drop(release_tx);
                let _ = worker.join();
                result.map(|_| None)
            }
        }
    }
}
impl Drop for Reservation {
    fn drop(&mut self) {
        drop(self.release.take());
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

struct MutexOwner(OwnedHandle);

impl MutexOwner {
    fn try_acquire(name: &str) -> Result<Option<Self>> {
        // Explicit owner rights prevent the creator from implicitly receiving WRITE_DAC.
        let sddl: Vec<u16> = "D:P(A;;GA;;;SY)(A;;GA;;;BA)(A;;RC;;;OW)(A;;0x00100001;;;AU)\0"
            .encode_utf16()
            .collect();
        let mut descriptor = std::ptr::null_mut();
        if unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                sddl.as_ptr(),
                SDDL_REVISION_1,
                &mut descriptor,
                std::ptr::null_mut(),
            )
        } == 0
        {
            return Err(std::io::Error::last_os_error())
                .context("could not create core execution mutex permissions");
        }
        let attributes = SECURITY_ATTRIBUTES {
            nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: descriptor,
            bInheritHandle: 0,
        };
        let wide: Vec<u16> = name.encode_utf16().chain(Some(0)).collect();
        let raw = unsafe { CreateMutexExW(&attributes, wide.as_ptr(), 0, 0x00100001) };
        let error = std::io::Error::last_os_error();
        unsafe {
            LocalFree(descriptor);
        }
        if raw.is_null() {
            return Err(error)
                .with_context(|| format!("could not open core execution mutex {name:?}"));
        }
        let handle = unsafe { OwnedHandle::from_raw_handle(raw) };
        match unsafe { WaitForSingleObject(handle.as_raw_handle(), 0) } {
            // The caller checks for surviving cores while holding the reservation.
            WAIT_OBJECT_0 | WAIT_ABANDONED => Ok(Some(Self(handle))),
            WAIT_TIMEOUT => Ok(None),
            _ => bail!(
                "could not reserve core execution mutex {name:?}: {}",
                std::io::Error::last_os_error()
            ),
        }
    }
}

impl Drop for MutexOwner {
    fn drop(&mut self) {
        unsafe {
            ReleaseMutex(self.0.as_raw_handle());
        }
    }
}
