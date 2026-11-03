#[cfg(any(not(target_os = "macos"), test))]
pub(super) use suki_service::proxy_owner::types::Endpoint;
pub(super) use suki_service::proxy_owner::types::{ManualProxy, OsSnapshot, PacProxy};
