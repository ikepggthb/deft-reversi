fn main() {
    for name in ["TARGET", "PROFILE", "OPT_LEVEL", "CARGO_ENCODED_RUSTFLAGS"] {
        let value = std::env::var(name).unwrap_or_default().replace('\x1f', " ");
        println!("cargo:rustc-env=DEFT_BUILD_{name}={value}");
    }
    println!("cargo:rerun-if-changed=build.rs");
}
