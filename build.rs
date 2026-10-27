fn main() {
    slint_build::compile("src/ui/main.slint").expect("Failed to compile Slint UI");

    #[cfg(windows)]
    {
        let mut res = winres::WindowsResource::new();
        res.set_icon("assets/icon.ico");
        res.set("ProductName", "Another Encoder");
        res.set("FileDescription", "Another Encoder — Professional Media Transcoder");
        if let Err(e) = res.compile() {
            eprintln!("Warning: Failed to compile windows resource: {}", e);
        }
    }
}
