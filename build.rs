use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-env-changed=SPANREED_NATIVE_NOTICES");
    println!("cargo:rerun-if-env-changed=SPANREED_NATIVE_NOTICES_REQUIRED");
    println!("cargo:rerun-if-changed=LICENSE");
    let destination = PathBuf::from(std::env::var_os("OUT_DIR").expect("Cargo output directory"))
        .join("native-notices.txt");
    let contents = match std::env::var_os("SPANREED_NATIVE_NOTICES") {
        Some(path) => {
            let path = PathBuf::from(path);
            println!("cargo:rerun-if-changed={}", path.display());
            let metadata = std::fs::symlink_metadata(&path).expect("Native notices file");
            assert!(metadata.is_file() && metadata.len() <= 32 * 1024 * 1024);
            let contents = std::fs::read_to_string(path).expect("Original UTF-8 native notices");
            assert!(contents.starts_with("Spanreed native third-party notices\n"));
            contents
        }
        None => {
            assert!(
                std::env::var_os("SPANREED_NATIVE_NOTICES_REQUIRED").is_none(),
                "Distribution builds require verified SPANREED_NATIVE_NOTICES"
            );
            "This local build does not include a qualified native dependency notice bundle. Generate one with scripts/native-notices.py before redistribution.\n".into()
        }
    };
    std::fs::write(destination, contents).expect("Embed native notices");
}
