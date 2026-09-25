//! `spanreed account …`

pub fn cmd(args: &[String]) -> std::process::ExitCode {
    match crate::drivers::dispatch_account(args) {
        Ok(out) => {
            if !out.is_empty() {
                print!("{out}");
                if !out.ends_with('\n') {
                    println!();
                }
            }
            std::process::ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("{e}");
            std::process::ExitCode::FAILURE
        }
    }
}
