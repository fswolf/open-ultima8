//! Windows builds: the game's icon (icon/open-u8.ico) built into u8view.exe, for Explorer and
//! the taskbar. (Elsewhere the window sets its icon itself.)
fn main() {
    let dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let ico = std::path::Path::new(&dir).join("../../icon/open-u8.ico");
    println!("cargo:rerun-if-changed={}", ico.display());
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let out = std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let rc = out.join("open-ultima8.rc");
    let path = ico.canonicalize().unwrap_or(ico).display().to_string().replace('\\', "/");
    std::fs::write(&rc, format!("1 ICON \"{path}\"\n")).unwrap();
    if let Err(e) = embed_resource::compile(&rc, embed_resource::NONE).manifest_optional() {
        println!("cargo:warning=u8view.exe gets no icon: {e}");
    }
}
