//! Embeds the icon and version information in the Windows executable, for Explorer, shortcuts
//! and the file's Properties. Does nothing when building for other platforms.

use std::path::Path;

fn main() {
    let icon = Path::new("../../packaging/windows/eleven.ico");
    println!("cargo:rerun-if-changed={}", icon.display());
    // The target, not the host: cross-compiling from Linux to Windows embeds them too.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let mut res = winresource::WindowsResource::new();
    res.set_icon(&icon.to_string_lossy())
        // Task Manager lists processes by FileDescription, which defaults to the long
        // package description.
        .set("FileDescription", "eleven");
    // Without a resource compiler (rc.exe or windres) the program still works; it just has
    // the generic icon. scripts/package-windows.ps1 checks that release builds have them.
    if let Err(e) = res.compile() {
        println!("cargo:warning=Windows resources not embedded: {e}");
    }
}
