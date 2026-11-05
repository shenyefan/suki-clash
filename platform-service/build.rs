fn main() {
    println!("cargo:rerun-if-changed=../assets/logo.ico");
    println!("cargo:rerun-if-changed=../packaging/windows/service.rc");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows")
        && std::env::var_os("CARGO_FEATURE_STANDALONE").is_some()
    {
        embed_resource::compile_for(
            "../packaging/windows/service.rc",
            ["suki-clash-service"],
            embed_resource::NONE,
        )
        .manifest_required()
        .expect("无法嵌入 Windows 服务图标");
    }
}
