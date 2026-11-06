fn main() {
    println!("cargo:rerun-if-changed=../assets/logo.ico");
    println!("cargo:rerun-if-changed=../packaging/windows/service.rc");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows")
        && std::env::var_os("CARGO_FEATURE_STANDALONE").is_some()
    {
        let icon = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../assets/logo.ico");
        let icon = format!(
            "SUKI_APP_ICON=\"{}\"",
            icon.display().to_string().replace('\\', "/")
        );
        embed_resource::compile_for(
            "../packaging/windows/service.rc",
            ["suki-clash-service"],
            [icon],
        )
        .manifest_required()
        .expect("无法嵌入 Windows 服务图标");
    }
}
