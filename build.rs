fn main() {
    // use different windres for windows vs cross-compilation
    let windres = if cfg!(target_os = "windows") {
        "windres"
    } else {
        "x86_64-w64-mingw32-windres"
    };

    // use windres to embed icon in exe on windows
    // place icon.res in cargo temp build directory instead of source foolder
    let out_dir = std::env::var("OUT_DIR").unwrap();
    let res_path = format!("{}/icon.res", out_dir);

    std::process::Command::new(windres)
        .args(["assets/icon.rc", "-O", "coff", "-o", &res_path])
        .status()
        .unwrap();

    println!("cargo:rustc-link-arg={}", res_path);
}
