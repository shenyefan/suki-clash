//! Subscription secrets never enter the metadata document, logs, or process arguments.
use crate::storage::AppPaths;
use sha2::{Digest as _, Sha256};
pub(crate) struct CredentialStore {
    service: String,
}
impl CredentialStore {
    pub(crate) fn new(paths: &AppPaths) -> Self {
        let hash = Sha256::digest(paths.data().to_string_lossy().as_bytes());
        Self {
            service: format!(
                "dev.suki.clash.profiles.{}",
                hash.iter()
                    .map(|byte| format!("{byte:02x}"))
                    .collect::<String>()
            ),
        }
    }
    pub(crate) fn overrides(paths: &AppPaths) -> Self {
        let mut store = Self::new(paths);
        store.service = store.service.replace(".profiles.", ".overrides.");
        store
    }
    pub(crate) async fn read(&self, id: u64) -> Result<(String, String), String> {
        #[cfg(all(target_os = "macos", not(test)))]
        {
            match security_framework::passwords::get_generic_password(
                &self.service,
                &id.to_string(),
            ) {
                Ok(bytes) => serde_json::from_slice(&bytes).map_err(|_| "订阅凭据格式无效".into()),
                Err(error) if error.code() == -25300 => Ok((String::new(), String::new())),
                Err(_) => Err("无法读取订阅钥匙串凭据".into()),
            }
        }
        #[cfg(all(windows, not(test)))]
        {
            windows::read(&self.service, id)
        }
        #[cfg(all(target_os = "linux", not(test)))]
        {
            linux::read(&self.service, id).await
        }
        #[cfg(test)]
        {
            let _ = (&self.service, id);
            Ok((String::new(), String::new()))
        }
    }
    pub(crate) async fn write(&self, id: u64, token: &str, age: &str) -> Result<(), String> {
        if token.is_empty() && age.is_empty() {
            return self.delete(id).await;
        }
        #[cfg(all(target_os = "macos", not(test)))]
        {
            let bytes = serde_json::to_vec(&(token, age)).map_err(|_| "无法编码订阅凭据")?;
            security_framework::passwords::set_generic_password(
                &self.service,
                &id.to_string(),
                &bytes,
            )
            .map_err(|_| "无法保存订阅钥匙串凭据".into())
        }
        #[cfg(all(windows, not(test)))]
        {
            windows::write(&self.service, id, token, age)
        }
        #[cfg(all(target_os = "linux", not(test)))]
        {
            linux::write(&self.service, id, token, age).await
        }
        #[cfg(test)]
        {
            let _ = (&self.service, id);
            if token.is_empty() && age.is_empty() {
                Ok(())
            } else {
                Err("当前平台尚未接入订阅凭据安全存储".into())
            }
        }
    }
    pub(crate) async fn restore(
        &self,
        previous: std::collections::BTreeMap<u64, (String, String)>,
    ) -> Result<(), String> {
        let mut failures = 0;
        for (id, (token, age)) in previous {
            if self.write(id, &token, &age).await.is_err() {
                failures += 1;
            }
        }
        if failures == 0 {
            Ok(())
        } else {
            Err(format!("{failures} 项订阅凭据恢复失败"))
        }
    }
    pub(crate) async fn delete(&self, id: u64) -> Result<(), String> {
        #[cfg(all(target_os = "macos", not(test)))]
        {
            match security_framework::passwords::delete_generic_password(
                &self.service,
                &id.to_string(),
            ) {
                Ok(()) => Ok(()),
                Err(error) if error.code() == -25300 => Ok(()),
                Err(_) => Err("无法删除订阅钥匙串凭据".into()),
            }
        }
        #[cfg(all(windows, not(test)))]
        {
            windows::delete(&self.service, id)
        }
        #[cfg(all(target_os = "linux", not(test)))]
        {
            linux::delete(&self.service, id).await
        }
        #[cfg(test)]
        {
            let _ = (&self.service, id);
            Ok(())
        }
    }
}

#[cfg(all(target_os = "linux", not(test)))]
#[path = "credentials_linux.rs"]
mod linux;
#[cfg(all(windows, not(test)))]
#[path = "credentials_windows.rs"]
mod windows;
