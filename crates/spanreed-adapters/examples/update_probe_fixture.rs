use std::io::Write;
use std::process::{Command, Stdio};
use std::time::Duration;

fn main() {
    let executable = std::env::current_exe().unwrap();
    if std::env::args().any(|arg| arg == "--hold") {
        std::thread::sleep(Duration::from_secs(2));
        std::fs::write(executable.with_extension("escaped"), b"unconfined").unwrap();
        std::thread::sleep(Duration::from_secs(30));
        return;
    }
    let scenario = executable.file_stem().unwrap().to_str().unwrap();
    if scenario.contains("marker") {
        std::fs::write(executable.with_extension("escaped"), b"executed").unwrap();
    }
    if scenario.contains("child") {
        #[allow(
            clippy::zombie_processes,
            reason = "Fixture tests descendant cleanup after its parent exits"
        )]
        let _ = Command::new(&executable)
            .arg("--hold")
            .stdin(Stdio::null())
            .stdout(Stdio::inherit())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
    }
    if scenario.contains("timeout") {
        std::thread::sleep(Duration::from_secs(30));
    }
    if scenario.contains("excess") {
        std::io::stdout().write_all(&[b'x'; 8192]).unwrap();
        return;
    }
    if scenario.contains("wrong") {
        println!("spanreed 9.9.9");
        return;
    }
    println!("spanreed 1.2.3");
}
