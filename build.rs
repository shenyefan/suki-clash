fn main() {
    println!("cargo:rerun-if-changed=assets/logo.ico");
    println!("cargo:rerun-if-changed=packaging/windows/app.rc");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        embed_resource::compile("packaging/windows/app.rc", embed_resource::NONE)
            .manifest_required()
            .expect("无法嵌入 Windows 应用图标");
    }
}
