//! Subscription secrets never enter the metadata document, logs, or process arguments.
use crate::storage::AppPaths;
use sha2::{Digest as _, Sha256};

#[derive(Clone, Default, PartialEq, Eq)]
pub(crate) enum SecretEdit {
    #[default]
    Keep,
    Replace(String),
    Clear,
}
impl std::fmt::Debug for SecretEdit {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Keep => "Keep",
            Self::Replace(_) => "Replace",
            Self::Clear => "Clear",
        })
    }
}
impl SecretEdit {
    fn resolve(&self, previous: String) -> String {
        match self {
            Self::Keep => previous,
            Self::Replace(value) => value.clone(),
            Self::Clear => String::new(),
        }
    }
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct CredentialEdits {
    pub(crate) token: SecretEdit,
    pub(crate) age: SecretEdit,
}
impl CredentialEdits {
    pub(crate) fn from_inputs(token: &str, age: &str) -> Self {
        let edit = |value: &str| {
            if value.is_empty() {
                SecretEdit::Keep
            } else {
                SecretEdit::Replace(value.to_owned())
            }
        };
        Self {
            token: edit(token),
            age: edit(age),
        }
    }
    pub(crate) fn changed(&self) -> bool {
        self.token != SecretEdit::Keep || self.age != SecretEdit::Keep
    }
    pub(crate) fn resolve(&self, previous: (String, String)) -> (String, String) {
        (self.token.resolve(previous.0), self.age.resolve(previous.1))
    }
}
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
    pub(crate) fn gist(paths: &AppPaths) -> Self {
        let mut store = Self::new(paths);
        store.service = store.service.replace(".profiles.", ".gist.");
        store
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
                Ok(bytes) => serde_json::from_slice(&bytes)
                    .map_err(|error| crate::logging::rejected("安全凭据格式无效".into(), error)),
                Err(error) if error.code() == -25300 => Ok((String::new(), String::new())),
                Err(error) => Err(crate::logging::diagnostic_error(
                    "无法读取钥匙串凭据".into(),
                    error,
                )),
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
            Ok(test_vault()
                .lock()
                .map_err(|error| crate::logging::diagnostic_error("隔离凭据存储不可用", error))?
                .entries
                .get(&(self.service.clone(), id))
                .cloned()
                .unwrap_or_default())
        }
    }
    pub(crate) async fn write(&self, id: u64, token: &str, age: &str) -> Result<(), String> {
        if token.is_empty() && age.is_empty() {
            return self.delete(id).await;
        }
        #[cfg(all(target_os = "macos", not(test)))]
        {
            let bytes = serde_json::to_vec(&(token, age))
                .map_err(|error| crate::logging::diagnostic_error("无法编码安全凭据", error))?;
            security_framework::passwords::set_generic_password(
                &self.service,
                &id.to_string(),
                &bytes,
            )
            .map_err(|error| crate::logging::diagnostic_error("无法保存钥匙串凭据".into(), error))
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
            test_vault()
                .lock()
                .map_err(|error| crate::logging::diagnostic_error("隔离凭据存储不可用", error))?
                .entries
                .insert(
                    (self.service.clone(), id),
                    (token.to_owned(), age.to_owned()),
                );
            Ok(())
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
                Err(error) => Err(crate::logging::diagnostic_error(
                    "无法删除钥匙串凭据".into(),
                    error,
                )),
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
            let mut store = test_vault()
                .lock()
                .map_err(|error| crate::logging::diagnostic_error("隔离凭据存储不可用", error))?;
            let key = (self.service.clone(), id);
            if store.failed_deletes.remove(&key) {
                return Err("隔离凭据清理失败".into());
            }
            store.entries.remove(&key);
            Ok(())
        }
    }
    #[cfg(test)]
    pub(crate) fn fail_next_delete(&self, id: u64) {
        test_vault()
            .lock()
            .unwrap()
            .failed_deletes
            .insert((self.service.clone(), id));
    }
}

#[cfg(all(target_os = "linux", not(test)))]
#[path = "credentials_linux.rs"]
mod linux;
#[cfg(all(windows, not(test)))]
#[path = "credentials_windows.rs"]
mod windows;

#[cfg(test)]
#[derive(Default)]
struct TestVault {
    entries: std::collections::BTreeMap<(String, u64), (String, String)>,
    failed_deletes: std::collections::BTreeSet<(String, u64)>,
}
#[cfg(test)]
fn test_vault() -> &'static std::sync::Mutex<TestVault> {
    static VAULT: std::sync::OnceLock<std::sync::Mutex<TestVault>> = std::sync::OnceLock::new();
    VAULT.get_or_init(Default::default)
}
