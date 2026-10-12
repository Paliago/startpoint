fn main() {
    println!("cargo:rerun-if-env-changed=STARTPOINT_RELEASE_REPOSITORY");
    println!("cargo:rerun-if-changed=windows.rc");
    println!("cargo:rerun-if-changed=assets/icons/startpoint.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        embed_resource::compile_for("windows.rc", ["startpoint"], embed_resource::NONE)
            .manifest_required()
            .expect("the Windows SDK resource compiler must embed the Startpoint icon");
    }
}
