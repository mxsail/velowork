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

        // Embed Windows application manifest for PerMonitorV2 HiDPI awareness and UTF-8 code page
        res.set_manifest(r#"
<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
    <trustInfo xmlns="urn:schemas-microsoft-com:asm.v3">
        <security>
            <requestedPrivileges>
                <requestedExecutionLevel level="asInvoker" uiAccess="false" />
            </requestedPrivileges>
        </security>
    </trustInfo>
    <compatibility xmlns="urn:schemas-microsoft-com:compatibility.v1">
        <application>
            <!-- Windows 10 and Windows 11 -->
            <supportedOS Id="{8e0f7a12-bfb3-4fe8-b9a5-48fd50a15a9a}" />
        </application>
    </compatibility>
    <application xmlns="urn:schemas-microsoft-com:asm.v3">
        <windowsSettings>
            <dpiAware xmlns="http://schemas.microsoft.com/SMI/2005/WindowsSettings">true/pm</dpiAware>
            <dpiAwareness xmlns="http://schemas.microsoft.com/SMI/2016/WindowsSettings">PerMonitorV2</dpiAwareness>
            <activeCodePage xmlns="http://schemas.microsoft.com/SMI/2019/WindowsSettings">UTF-8</activeCodePage>
        </windowsSettings>
    </application>
    <dependency>
        <dependentAssembly>
            <assemblyIdentity
                type='win32'
                name='Microsoft.Windows.Common-Controls'
                version='6.0.0.0'
                processorArchitecture='*'
                publicKeyToken='6595b64144ccf1df'
            />
        </dependentAssembly>
    </dependency>
</assembly>
"#);

        if let Err(e) = res.compile() {
            eprintln!("Warning: Failed to compile Windows resource: {}", e);
        }
    }

    // Rerun if icon files change
    println!("cargo:rerun-if-changed=assets/app-icon.ico");
    println!("cargo:rerun-if-changed=assets/logo.png");
    println!("cargo:rerun-if-changed=assets/velowork_icon.svg");
}
