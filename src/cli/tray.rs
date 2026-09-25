//! `spanreed tray [--interval S]` (feature `tray`).

use std::process::ExitCode;

use crate::app::AppContext;

#[cfg(feature = "tray")]
pub(super) fn run(ctx: &AppContext, args: &[String]) -> ExitCode {
    use spanreed_tray::DEFAULT_INTERVAL_SECS;
    let mut interval = DEFAULT_INTERVAL_SECS;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--interval" => {
                i += 1;
                let s = args
                    .get(i)
                    .ok_or_else(|| "--interval needs a value".to_string())
                    .and_then(|v| v.parse::<u64>().map_err(|_| format!("bad --interval: {v}")));
                match s {
                    Ok(n) if n >= 5 => interval = n,
                    Ok(_) => {
                        eprintln!("tray: --interval minimum is 5s");
                        return ExitCode::FAILURE;
                    }
                    Err(e) => {
                        eprintln!("tray: {e}");
                        return ExitCode::FAILURE;
                    }
                }
            }
            "-h" | "--help" => {
                println!(
                    "spanreed tray — system tray status (Spanreed icon)\n\n\
                     \t--interval S   Refresh every S seconds (default {DEFAULT_INTERVAL_SECS})\n\
                     Left click opens the usage card. Right click keeps the menu:\n\
                     \tOpen dashboard, Settings, Refresh, Ensure, Open log,\n\
                     \tLink/Share/Unlink, Check/Install update, Quit tray"
                );
                return ExitCode::SUCCESS;
            }
            other => {
                eprintln!("tray: unknown arg: {other}");
                return ExitCode::FAILURE;
            }
        }
        i += 1;
    }
    if let Err(e) = spanreed_tray::run(ctx.clone(), interval) {
        eprintln!("tray: {e}");
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

#[cfg(not(feature = "tray"))]
pub(super) fn run(_ctx: &AppContext, _args: &[String]) -> ExitCode {
    eprintln!(
        "tray: this binary was built without the `tray` feature\n\
         rebuild with: cargo build --release --features tray"
    );
    ExitCode::FAILURE
}
