// use windres to embed icon in exe on windows
fn main() {
    // for cross-compilation from linux
    println!("cargo:rustc-link-arg=assets/icon.res");
    
    std::process::Command::new("x86_64-w64-mingw32-windres")
        .args(["assets/icon.rc", "-O", "coff", "-o", "assets/icon.res"])
        .status()
        .unwrap();
}