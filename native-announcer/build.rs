fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        println!("cargo:rerun-if-changed=resources/herald.ico");
        winresource::WindowsResource::new()
            .set_icon("resources/herald.ico")
            .compile()
            .expect("could not compile Herald icon");
        println!("cargo:rustc-link-arg=/DEBUG:NONE");
        println!("cargo:rustc-link-arg=/DELAYLOAD:sherpa-onnx-c-api.dll");
        println!("cargo:rustc-link-lib=delayimp");
    }
}
