fn main() {
    // use different windres for windows vs cross-compilation
    let windres = if cfg!(target_os = "windows") {
        "windres"
    } else {
        "x86_64-w64-mingw32-windres"
    };

    // read .env file if it exists and pass values to compiler if built locally
    // if built in pipeline, this block will be ignored and github actions secret will be used 
    if let Ok(contents) = std::fs::read_to_string(".env") {
        for line in contents.lines() {
            let line = line.trim();
            // skip blank lines and comments
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if let Some((key, val)) = line.split_once('=') {
                println!("cargo:rustc-env={}={}", key.trim(), val.trim());
            }
        }
    }

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
