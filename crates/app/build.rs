fn main() {
    #[cfg(target_os = "windows")]
    {
        if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
            return;
        }
        let mut res = winresource::WindowsResource::new();
        res.set_icon("../../assets/plotx.ico");
        res.compile().expect("embed Windows icon resource");
    }
}
