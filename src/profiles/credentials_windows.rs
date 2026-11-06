//! Current-user Windows Credential Manager; credentials never enter settings files.
use windows_sys::Win32::{
    Foundation::{ERROR_NOT_FOUND, GetLastError},
    Security::Credentials::*,
};
fn target(service: &str, id: u64) -> Vec<u16> {
    format!("{service}/{id}")
        .encode_utf16()
        .chain(Some(0))
        .collect()
}
pub(super) fn read(service: &str, id: u64) -> Result<(String, String), String> {
    let target = target(service, id);
    let mut pointer = std::ptr::null_mut();
    unsafe {
        if CredReadW(target.as_ptr(), CRED_TYPE_GENERIC, 0, &mut pointer) == 0 {
            return if GetLastError() == ERROR_NOT_FOUND {
                Ok((String::new(), String::new()))
            } else {
                Err(crate::logging::diagnostic_error(
                    "无法读取 Windows 订阅凭据".into(),
                    std::io::Error::last_os_error(),
                ))
            };
        }
        let credential = &*pointer;
        let bytes = if credential.CredentialBlobSize == 0 {
            &[]
        } else {
            std::slice::from_raw_parts(
                credential.CredentialBlob,
                credential.CredentialBlobSize as usize,
            )
        };
        let result = serde_json::from_slice(bytes)
            .map_err(|error| crate::logging::rejected("订阅凭据格式无效".into(), error));
        CredFree(pointer.cast());
        result
    }
}
pub(super) fn write(service: &str, id: u64, token: &str, age: &str) -> Result<(), String> {
    let mut target = target(service, id);
    let mut bytes = serde_json::to_vec(&(token, age))
        .map_err(|error| crate::logging::diagnostic_error("无法编码订阅凭据", error))?;
    let credential = CREDENTIALW {
        Type: CRED_TYPE_GENERIC,
        TargetName: target.as_mut_ptr(),
        CredentialBlobSize: bytes.len() as u32,
        CredentialBlob: bytes.as_mut_ptr(),
        Persist: CRED_PERSIST_LOCAL_MACHINE,
        ..Default::default()
    };
    if unsafe { CredWriteW(&credential, 0) } == 0 {
        Err(crate::logging::diagnostic_error(
            "无法保存 Windows 订阅凭据".into(),
            std::io::Error::last_os_error(),
        ))
    } else {
        Ok(())
    }
}
pub(super) fn delete(service: &str, id: u64) -> Result<(), String> {
    let target = target(service, id);
    if unsafe { CredDeleteW(target.as_ptr(), CRED_TYPE_GENERIC, 0) } != 0
        || unsafe { GetLastError() } == ERROR_NOT_FOUND
    {
        Ok(())
    } else {
        Err(crate::logging::diagnostic_error(
            "无法删除 Windows 订阅凭据".into(),
            std::io::Error::last_os_error(),
        ))
    }
}
