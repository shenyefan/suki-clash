//! Snapshot collection happens off the UI thread. No inferred service success.
#[derive(Clone)]
pub(crate) struct Info {
    pub(crate) os: String,
    pub(crate) architecture: String,
    pub(crate) hostname: String,
    pub(crate) elevated: Result<bool, String>,
    pub(crate) autostart: Result<bool, String>,
}
impl Info {
    pub(crate) fn collect() -> Self {
        Self {
            os: sysinfo::System::long_os_version()
                .or_else(|_| sysinfo::System::name())
                .unwrap_or_else(|_| std::env::consts::OS.into()),
            architecture: std::env::consts::ARCH.into(),
            hostname: sysinfo::System::host_name().unwrap_or_else(|_| "未知".into()),
            elevated: crate::platform_access::elevated(),
            autostart: super::autostart::enabled(),
        }
    }
}
