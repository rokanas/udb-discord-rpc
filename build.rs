// use windres to embed icon in exe on windows
fn main() {
    // place icon.res in cargo temp build directory instead of source foolder
    let out_dir = std::env::var("OUT_DIR").unwrap();
    let res_path = format!("{}/icon.res", out_dir);

    std::process::Command::new("x86_64-w64-mingw32-windres")
        .args(["assets/icon.rc", "-O", "coff", "-o", &res_path])
        .status()
        .unwrap();

    println!("cargo:rustc-link-arg={}", res_path);
}
