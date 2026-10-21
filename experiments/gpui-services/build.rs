fn main() {
    println!("cargo:rerun-if-changed=src/services.m");
    #[cfg(target_os = "macos")]
    {
        cc::Build::new()
            .file("src/services.m")
            .flag("-fobjc-arc")
            .compile("spqx_services_probe");
        println!("cargo:rustc-link-lib=framework=AppKit");
        println!("cargo:rustc-link-lib=objc");
    }
}
