fn main() {
    println!("cargo:rerun-if-changed=src/ui");
    println!("cargo:rerun-if-changed=assets/icon.ico");

    slint_build::compile("src/ui/main.slint").expect("Failed to compile Slint UI");

    #[cfg(windows)]
    {
        let version = env!("CARGO_PKG_VERSION");
        let mut res = winres::WindowsResource::new();
        res.set_icon("assets/icon.ico");
        res.set("ProductName", "Kabosu Converter");
        res.set("FileDescription", "Kabosu Converter — Professional Media Transcoder");
        res.set("ProductVersion", version);
        res.set("FileVersion", version);
        res.set("LegalCopyright", "Copyright (c) SThao");
        if let Err(e) = res.compile() {
            eprintln!("Warning: Failed to compile windows resource: {}", e);
        }
    }
}
