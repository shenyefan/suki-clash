//! Windows kernel-level lifetime containment, equivalent to CV's sidecar Job Object.
#[cfg(windows)]
pub(super) struct Lifetime(std::os::windows::io::OwnedHandle);
#[cfg(not(windows))]
pub(super) struct Lifetime;
impl Lifetime {
    pub(super) fn attach(child: &tokio::process::Child) -> Result<Self, String> {
        #[cfg(windows)]
        {
            use std::os::windows::io::FromRawHandle as _;
            use windows_sys::Win32::System::JobObjects::{
                AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
                JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
                SetInformationJobObject,
            };
            unsafe {
                let job = CreateJobObjectW(std::ptr::null(), std::ptr::null());
                if job.is_null() {
                    return Err("无法建立内核进程生命周期保护".into());
                }
                let owned = std::os::windows::io::OwnedHandle::from_raw_handle(job);
                let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
                limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
                if SetInformationJobObject(
                    job,
                    JobObjectExtendedLimitInformation,
                    (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                    std::mem::size_of_val(&limits) as u32,
                ) == 0
                {
                    return Err("无法配置内核退出保护".into());
                }
                let process = child.raw_handle().ok_or("内核进程句柄不可用")?;
                if AssignProcessToJobObject(job, process) == 0 {
                    return Err("无法将内核加入本应用生命周期保护".into());
                }
                Ok(Self(owned))
            }
        }
        #[cfg(not(windows))]
        {
            let _ = child;
            Ok(Self)
        }
    }
}
#[cfg(windows)]
impl Drop for Lifetime {
    fn drop(&mut self) {
        use std::os::windows::io::AsRawHandle as _;
        let _ = self.0.as_raw_handle();
    }
}
