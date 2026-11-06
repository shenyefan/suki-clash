fn main() {
    println!("cargo:rerun-if-changed=assets/logo.ico");
    println!("cargo:rerun-if-changed=packaging/windows/app.rc");
    println!("cargo:rerun-if-changed=packaging/windows/version.rc");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let icon = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/logo.ico");
        let icon = format!(
            "SUKI_APP_ICON=\"{}\"",
            icon.display().to_string().replace('\\', "/")
        );
        embed_resource::compile(
            "packaging/windows/app.rc",
            [
                icon,
                format!("SUKI_VERSION_MAJOR={}", env!("CARGO_PKG_VERSION_MAJOR")),
                format!("SUKI_VERSION_MINOR={}", env!("CARGO_PKG_VERSION_MINOR")),
                format!("SUKI_VERSION_PATCH={}", env!("CARGO_PKG_VERSION_PATCH")),
                format!("SUKI_VERSION=\"{}\"", env!("CARGO_PKG_VERSION")),
            ],
        )
        .manifest_required()
        .expect("无法嵌入 Windows 应用资源");
    }
}
