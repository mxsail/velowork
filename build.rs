fn main() {
    // Windows: Embed icon into executable
    #[cfg(target_os = "windows")]
    {
        let icon_path = "assets/app-icon.ico";

        let mut res = winresource::WindowsResource::new();
        if std::path::Path::new(icon_path).exists() {
            res.set_icon(icon_path);
        } else {
            println!("cargo:warning=Icon file not found at {}, skipping Windows icon embedding", icon_path);
        }

        res.set("FileDescription", "Velowork");
        res.set("ProductName", "Velowork");

        // Note: Do NOT call res.set_manifest here. GPUI (crates/gpui/build.rs) already
        // embeds a Windows application manifest (resources/windows/gpui.manifest.xml)
        // with PerMonitorV2 HiDPI awareness, SegmentHeap, Common-Controls 6.0, and OS
        // compatibility. Calling set_manifest here results in duplicate RT_MANIFEST
        // resources and causes MSVC link.exe to fail with CVTRES CVT1100 / LNK1123.

        if let Err(e) = res.compile() {
            eprintln!("Warning: Failed to compile Windows resource: {}", e);
        }
    }

    // Rerun if icon files change
    println!("cargo:rerun-if-changed=assets/app-icon.ico");
    println!("cargo:rerun-if-changed=assets/logo.png");
    println!("cargo:rerun-if-changed=assets/velowork_icon.svg");
}
