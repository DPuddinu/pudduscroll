fn main() {
    println!("cargo:rerun-if-changed=assets/icon.ico");
    #[cfg(windows)]
    {
        let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR");
        let icon = std::path::Path::new(&manifest_dir)
            .join("assets")
            .join("icon.ico");
        let icon = icon.to_str().expect("icon path");
        winresource::WindowsResource::new()
            .set_icon(icon)
            .compile()
            .expect("embed Windows icon");
    }
}
