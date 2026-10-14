use std::io::Write;
use std::process::ExitCode;

pub(super) fn print(args: &[String]) -> ExitCode {
    if !args.is_empty() {
        eprintln!("usage: spanreed licenses");
        return ExitCode::FAILURE;
    }
    let mut output = std::io::stdout().lock();
    let result = output
        .write_all(include_bytes!("../../LICENSE"))
        .and_then(|()| output.write_all(b"\n\n"))
        .and_then(|()| {
            output.write_all(include_bytes!(concat!(
                env!("OUT_DIR"),
                "/native-notices.txt"
            )))
        });
    if result.is_ok() {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

pub(super) fn run(_ctx: &crate::app::AppContext, args: &[String]) -> ExitCode {
    print(args)
}
