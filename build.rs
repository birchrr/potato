// The exe's own icon (Explorer, taskbar). Only when building on Windows, where the resource compiler is.
fn main() {
    #[cfg(windows)]
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        winresource::WindowsResource::new().set_icon("assets/icon.ico").compile().expect("embed the icon");
    }
    println!("cargo:rerun-if-changed=assets/icon.ico");
}
