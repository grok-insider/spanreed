fn main() {
    println!("cargo:rerun-if-env-changed=SPANREED_NATIVE_NOTICES");
    println!("cargo:rerun-if-env-changed=SPANREED_NATIVE_NOTICES_REQUIRED");
    let required = std::env::var_os("SPANREED_NATIVE_NOTICES_REQUIRED").is_some();
    let native = match std::env::var_os("SPANREED_NATIVE_NOTICES") {
        Some(path) => {
            let path = std::path::PathBuf::from(path);
            println!("cargo:rerun-if-changed={}", path.display());
            let metadata = std::fs::symlink_metadata(&path).expect("Native notice input");
            assert!(metadata.is_file() && metadata.len() <= 32 * 1024 * 1024);
            let bytes = std::fs::read(&path).expect("Original native notice bytes");
            assert!(bytes.starts_with(b"Spanreed native third-party notices\n"));
            bytes
        }
        None => {
            assert!(
                !required,
                "Desktop distribution requires qualified native notices"
            );
            b"Unqualified local desktop build: native dependency notices were not supplied.\n"
                .to_vec()
        }
    };
    let frontend = std::path::Path::new("../dist/FRONTEND-NOTICES.txt");
    println!("cargo:rerun-if-changed={}", frontend.display());
    let frontend = match std::fs::read(frontend) {
        Ok(bytes) => {
            assert!(bytes.len() <= 16 * 1024 * 1024);
            assert!(bytes.starts_with(b"Spanreed desktop frontend notices\n"));
            bytes
        }
        Err(_) => {
            assert!(
                !required,
                "Desktop distribution requires original frontend notices"
            );
            b"Unqualified local desktop build: frontend dependency notices were not generated.\n"
                .to_vec()
        }
    };
    let out =
        std::path::PathBuf::from(std::env::var_os("OUT_DIR").expect("Cargo output directory"));
    std::fs::write(out.join("native-notices.txt"), native).expect("Embed native notices");
    std::fs::write(out.join("frontend-notices.txt"), frontend).expect("Embed frontend notices");
    tauri_build::build()
}
