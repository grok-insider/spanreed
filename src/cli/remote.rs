use std::process::ExitCode;

pub(super) fn run(_: &crate::app::AppContext, args: &[String]) -> ExitCode {
    spanreed_remote::run(args)
}
