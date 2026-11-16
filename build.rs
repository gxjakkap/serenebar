// Puts icons/serenebar.ico into the exe as resource 1, for Explorer, the taskbar and our windows.
fn main() {
    println!("cargo:rerun-if-changed=icons/serenebar.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        embed_resource::compile("icons/serenebar.rc", embed_resource::NONE)
            .manifest_optional()
            .unwrap();
    }
}
