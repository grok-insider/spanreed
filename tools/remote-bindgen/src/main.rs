fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<std::ffi::OsString> = std::env::args_os().skip(1).collect();
    if args.len() != 2 {
        return Err("usage: remote-bindgen <wasm> <output-directory>".into());
    }
    wasm_bindgen_cli_support::Bindgen::new()
        .input_path(&args[0])
        .web(true)?
        .generate(&args[1])?;
    Ok(())
}
