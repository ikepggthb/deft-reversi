//! Embeds the application icon and version information in the Windows
//! executable. GPUI loads icon resource 1 as the window icon.
//!
//! `assets/deft-reversi.ico` is made from `assets/logo/` at the workspace
//! root: `svg/favicon-16.svg` for 16 px and `svg/deft-mark-light.svg` for
//! 20 to 256 px, stored as PNG entries.
fn main() {
    println!("cargo:rerun-if-changed=assets/deft-reversi.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut resource = winresource::WindowsResource::new();
        resource.set_icon("assets/deft-reversi.ico");
        resource.set("ProductName", "Deft Reversi");
        resource.set("FileDescription", "Deft Reversi");
        resource
            .compile()
            .expect("failed to embed the Windows icon and version information");
    }
}
