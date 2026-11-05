//! Linux Secret Service (or the encrypted desktop portal backend) through oo7.
fn attributes(service: &str, id: u64) -> [(String, String); 2] {
    [
        ("service".into(), service.into()),
        ("account".into(), id.to_string()),
    ]
}
async fn vault() -> Result<oo7::Keyring, String> {
    let vault = oo7::Keyring::new().await.map_err(|error| {
        crate::logging::diagnostic_error(
            "无法连接桌面密钥服务，请检查 Secret Service 或桌面门户",
            error,
        )
    })?;
    vault
        .unlock()
        .await
        .map_err(|error| crate::logging::diagnostic_error("桌面密钥库未解锁", error))?;
    Ok(vault)
}
pub(super) async fn read(service: &str, id: u64) -> Result<(String, String), String> {
    let vault = vault().await?;
    let items = vault
        .search_items(&attributes(service, id))
        .await
        .map_err(|error| crate::logging::diagnostic_error("无法读取订阅凭据", error))?;
    let Some(item) = items.first() else {
        return Ok((String::new(), String::new()));
    };
    let secret = item
        .secret()
        .await
        .map_err(|error| crate::logging::diagnostic_error("无法读取订阅密钥", error))?;
    serde_json::from_slice(secret.as_bytes())
        .map_err(|error| crate::logging::diagnostic_error("订阅凭据格式无效".into(), error))
}
pub(super) async fn write(service: &str, id: u64, token: &str, age: &str) -> Result<(), String> {
    let bytes = serde_json::to_vec(&(token, age))
        .map_err(|error| crate::logging::diagnostic_error("无法编码订阅凭据", error))?;
    vault()
        .await?
        .create_item("Suki Clash 订阅凭据", &attributes(service, id), bytes, true)
        .await
        .map_err(|error| crate::logging::diagnostic_error("无法保存订阅凭据".into(), error))
}
pub(super) async fn delete(service: &str, id: u64) -> Result<(), String> {
    vault()
        .await?
        .delete(&attributes(service, id))
        .await
        .map_err(|error| crate::logging::diagnostic_error("无法删除订阅凭据".into(), error))
}
