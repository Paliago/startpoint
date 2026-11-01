fn main() {
    println!("cargo:rerun-if-env-changed=STARTPOINT_RELEASE_REPOSITORY");
    println!("cargo:rerun-if-changed=windows.rc");
    println!("cargo:rerun-if-changed=assets/icons/startpoint.ico");
    println!("cargo:rerun-if-changed=assets/icons/startpoint-dev.ico");
    let dev = std::env::var_os("CARGO_CFG_DEBUG_ASSERTIONS").is_some()
        || std::env::var_os("CARGO_FEATURE_DEV_ICON").is_some();
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let macros = if dev {
            &["STARTPOINT_DEV_ICON"][..]
        } else {
            &[]
        };
        embed_resource::compile_for("windows.rc", ["startpoint"], macros)
            .manifest_required()
            .expect("the Windows SDK resource compiler must embed the Startpoint icon");
    }
}
