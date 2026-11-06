fn main() {
    println!("cargo:rerun-if-changed=../assets/logo.ico");
    println!("cargo:rerun-if-changed=../packaging/windows/service.rc");
    println!("cargo:rerun-if-changed=../packaging/windows/version.rc");
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
            [
                icon,
                format!("SUKI_VERSION_MAJOR={}", env!("CARGO_PKG_VERSION_MAJOR")),
                format!("SUKI_VERSION_MINOR={}", env!("CARGO_PKG_VERSION_MINOR")),
                format!("SUKI_VERSION_PATCH={}", env!("CARGO_PKG_VERSION_PATCH")),
                format!("SUKI_VERSION=\"{}\"", env!("CARGO_PKG_VERSION")),
            ],
        )
        .manifest_required()
        .expect("无法嵌入 Windows 服务资源");
    }
}
