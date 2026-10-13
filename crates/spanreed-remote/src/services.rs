use std::{
    path::{Path, PathBuf},
    process::Command,
};

pub(super) fn command_args(
    origin: Option<&str>,
    web_origin: Option<&str>,
) -> Result<Vec<String>, String> {
    if let Some(value) = web_origin {
        fabrials_agent_host::origin::validate_hosted_web_origin(value)
            .map_err(|_| "invalid_hosted_web_origin")?;
        if origin.is_none() {
            return Err("web_origin_requires_private_origin".into());
        }
    }
    match origin {
        None => Ok(vec!["remote".into(), "serve".into()]),
        Some(value) => {
            fabrials_agent_host::origin::validate_private_origin(value)
                .map_err(|_| "invalid_private_origin")?;
            let mut args = vec!["remote".into(), "private-serve".into(), value.into()];
            if let Some(web_origin) = web_origin {
                args.extend(["--web-origin".into(), web_origin.into()]);
            }
            Ok(args)
        }
    }
}

pub fn systemd(
    program: &Path,
    origin: Option<&str>,
    web_origin: Option<&str>,
) -> Result<String, String> {
    let args = command_args(origin, web_origin)?.join(" ");
    let path = program.to_str().ok_or("invalid_program_path")?;
    if path.chars().any(|ch| ch.is_control()) {
        return Err("invalid_program_path".into());
    }
    let path = path
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('%', "%%")
        .replace('$', "$$");
    Ok(format!(
        "[Unit]\nDescription=Spanreed Grok Remote\nAfter=network-online.target\n\n[Service]\nType=simple\nExecStart=\"{path}\" {args}\nRestart=on-failure\nRestartSec=5\nUMask=0077\nTimeoutStopSec=15\n\n[Install]\nWantedBy=default.target\n"
    ))
}

pub fn launch_agent(
    program: &Path,
    origin: Option<&str>,
    web_origin: Option<&str>,
) -> Result<String, String> {
    let args = command_args(origin, web_origin)?
        .iter()
        .map(|arg| format!("<string>{arg}</string>"))
        .collect::<String>();
    let path = program.to_str().ok_or("invalid_program_path")?;
    if path.chars().any(|ch| ch.is_control()) {
        return Err("invalid_program_path".into());
    }
    let path = path
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;");
    Ok(format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<plist version=\"1.0\"><dict><key>Label</key><string>com.fabrials.spanreed.remote</string><key>ProgramArguments</key><array><string>{path}</string>{args}</array><key>RunAtLoad</key><true/><key>KeepAlive</key><dict><key>SuccessfulExit</key><false/></dict><key>ThrottleInterval</key><integer>5</integer><key>ProcessType</key><string>Background</string></dict></plist>\n"
    ))
}

pub fn windows_task(
    program: &Path,
    origin: Option<&str>,
    web_origin: Option<&str>,
) -> Result<String, String> {
    let args = command_args(origin, web_origin)?.join(" ");
    let path = program.to_str().ok_or("invalid_program_path")?;
    if path.chars().any(|ch| ch.is_control()) {
        return Err("invalid_program_path".into());
    }
    let path = path.replace('\'', "''");
    Ok(format!(
        "$ErrorActionPreference = 'Stop'\n$remoteIdentity = [System.Security.Principal.WindowsIdentity]::GetCurrent()\n$remoteUser = $remoteIdentity.Name\n$remoteName = 'Spanreed Remote ' + $remoteIdentity.User.Value\n$remoteAction = New-ScheduledTaskAction -Execute '{path}' -Argument '{args}'\n$remoteTrigger = New-ScheduledTaskTrigger -AtLogOn -User $remoteUser\n$remotePrincipal = New-ScheduledTaskPrincipal -UserId $remoteUser -LogonType Interactive -RunLevel Limited\n$remoteSettings = New-ScheduledTaskSettingsSet -RestartCount 10 -RestartInterval (New-TimeSpan -Minutes 1) -ExecutionTimeLimit ([TimeSpan]::Zero) -MultipleInstances IgnoreNew -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries\nRegister-ScheduledTask -TaskName $remoteName -Action $remoteAction -Trigger $remoteTrigger -Principal $remotePrincipal -Settings $remoteSettings -Force | Out-Null\nStart-ScheduledTask -TaskName $remoteName\n"
    ))
}

fn windows_remove() -> String {
    "$ErrorActionPreference = 'Stop'\n$remoteName = 'Spanreed Remote ' + [System.Security.Principal.WindowsIdentity]::GetCurrent().User.Value\n$remoteTask = Get-ScheduledTask -TaskName $remoteName -ErrorAction SilentlyContinue\nif ($null -ne $remoteTask) { Stop-ScheduledTask -TaskName $remoteName; Unregister-ScheduledTask -TaskName $remoteName -Confirm:$false }\n".into()
}

#[derive(Debug, PartialEq, Eq)]
enum Step {
    Write(PathBuf, String),
    Remove(PathBuf),
    Run(String, Vec<String>),
}
#[derive(Clone, Copy)]
enum Platform {
    Linux,
    Mac,
    Windows,
}

fn plan(
    platform: Platform,
    install: bool,
    base: &Path,
    program: &Path,
    uid: Option<u32>,
    origin: Option<&str>,
    web_origin: Option<&str>,
) -> Result<Vec<Step>, String> {
    let run = |command: &str, args: Vec<String>| Step::Run(command.into(), args);
    match platform {
        Platform::Linux => {
            let file = base.join("systemd/user/spanreed-remote.service");
            if install {
                Ok(vec![
                    Step::Write(file, systemd(program, origin, web_origin)?),
                    run("systemctl", vec!["--user".into(), "daemon-reload".into()]),
                    run(
                        "systemctl",
                        vec![
                            "--user".into(),
                            "enable".into(),
                            "--now".into(),
                            "spanreed-remote.service".into(),
                        ],
                    ),
                ])
            } else {
                Ok(vec![
                    run(
                        "systemctl",
                        vec![
                            "--user".into(),
                            "disable".into(),
                            "--now".into(),
                            "spanreed-remote.service".into(),
                        ],
                    ),
                    Step::Remove(file),
                    run("systemctl", vec!["--user".into(), "daemon-reload".into()]),
                ])
            }
        }
        Platform::Mac => {
            let domain = format!("gui/{}", uid.ok_or("user_id_unavailable")?);
            let file = base.join("Library/LaunchAgents/com.fabrials.spanreed.remote.plist");
            let text = file.to_str().ok_or("invalid_service_path")?.to_owned();
            if install {
                Ok(vec![
                    Step::Write(file, launch_agent(program, origin, web_origin)?),
                    run("launchctl", vec!["bootstrap".into(), domain, text]),
                ])
            } else {
                Ok(vec![
                    run(
                        "launchctl",
                        vec![
                            "bootout".into(),
                            format!("{domain}/com.fabrials.spanreed.remote"),
                        ],
                    ),
                    Step::Remove(file),
                ])
            }
        }
        Platform::Windows => Ok(vec![run(
            "powershell.exe",
            vec![
                "-NoProfile".into(),
                "-NonInteractive".into(),
                "-Command".into(),
                if install {
                    windows_task(program, origin, web_origin)?
                } else {
                    windows_remove()
                },
            ],
        )]),
    }
}

fn execute(steps: Vec<Step>) -> Result<(), String> {
    for step in steps {
        match step {
            Step::Write(path, value) => {
                let parent = path.parent().ok_or("invalid_service_path")?;
                std::fs::create_dir_all(parent).map_err(|_| "service_directory_unwritable")?;
                std::fs::write(path, value).map_err(|_| "service_configuration_unwritable")?;
            }
            Step::Remove(path) => {
                if let Err(error) = std::fs::remove_file(path)
                    && error.kind() != std::io::ErrorKind::NotFound
                {
                    return Err("service_configuration_unwritable".into());
                }
            }
            Step::Run(command, args) => {
                let status = Command::new(&command)
                    .args(&args)
                    .status()
                    .map_err(|_| format!("{command}_unavailable"))?;
                if !status.success() {
                    return Err(format!("{command}_failed_check_user_service_permissions"));
                }
            }
        }
    }
    Ok(())
}

fn parse_args(args: &[String]) -> Result<(&str, Option<&str>, Option<&str>), String> {
    let Some(action) = args.first().map(String::as_str) else {
        return Err("missing_service_action".into());
    };
    if !matches!(action, "print" | "install" | "uninstall")
        || (action == "uninstall" && args.len() != 1)
        || args.len().is_multiple_of(2)
    {
        return Err("invalid_service_arguments".into());
    }
    let mut origin = None;
    let mut web_origin = None;
    for pair in args[1..].as_chunks::<2>().0 {
        match pair[0].as_str() {
            "--private-origin" if origin.is_none() => origin = Some(pair[1].as_str()),
            "--web-origin" if web_origin.is_none() => web_origin = Some(pair[1].as_str()),
            _ => return Err("invalid_service_arguments".into()),
        }
    }
    Ok((action, origin, web_origin))
}

pub fn run(args: &[String]) -> Result<(), String> {
    if std::iter::once(fabrials_agent_host::state_dir::STATE_DIR_ENV)
        .chain(fabrials_agent_host::state_dir::LEGACY_STATE_DIR_ENVS)
        .any(|name| std::env::var_os(name).is_some())
    {
        return Err("remote_service_state_override_unsupported: service print, install and uninstall do not preserve FABRIALS_AGENT_STATE_DIR or legacy state overrides; use private-serve in the foreground for isolated state. No service configuration or running service was changed".into());
    }
    let (action, origin, web_origin) = parse_args(args)?;
    command_args(origin, web_origin)?;
    if action == "install"
        && let Some(endpoint) = origin
    {
        let directory = crate::directory()?;
        fabrials_remote_host::direct::validate_configuration(
            &directory,
            endpoint,
            web_origin.unwrap_or(fabrials_agent_host::origin::PRODUCTION_WEB_ORIGIN),
        )?;
    }
    let program = std::env::current_exe().map_err(|_| "program_path_unavailable")?;
    let platform = if cfg!(target_os = "windows") {
        Platform::Windows
    } else if cfg!(target_os = "macos") {
        Platform::Mac
    } else {
        Platform::Linux
    };
    if args[0] == "print" {
        print!(
            "{}",
            match platform {
                Platform::Windows => windows_task(&program, origin, web_origin)?,
                Platform::Mac => launch_agent(&program, origin, web_origin)?,
                Platform::Linux => systemd(&program, origin, web_origin)?,
            }
        );
        return Ok(());
    }
    let install = match args[0].as_str() {
        "install" => true,
        "uninstall" => false,
        _ => return Err("usage: spanreed remote service print|install|uninstall".into()),
    };
    let (base, uid) = match platform {
        Platform::Windows => (PathBuf::new(), None),
        Platform::Linux => {
            let base = std::env::var_os("XDG_CONFIG_HOME")
                .map(PathBuf::from)
                .filter(|path| path.is_absolute())
                .or_else(|| {
                    std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config"))
                })
                .ok_or("user_home_unavailable")?;
            (base, None)
        }
        Platform::Mac => {
            let home = std::env::var_os("HOME").ok_or("user_home_unavailable")?;
            let output = Command::new("/usr/bin/id")
                .arg("-u")
                .output()
                .map_err(|_| "user_id_unavailable")?;
            if !output.status.success() {
                return Err("user_id_unavailable".into());
            }
            let uid = String::from_utf8(output.stdout)
                .map_err(|_| "user_id_unavailable")?
                .trim()
                .parse::<u32>()
                .map_err(|_| "user_id_unavailable")?;
            (PathBuf::from(home), Some(uid))
        }
    };
    execute(plan(
        platform, install, &base, &program, uid, origin, web_origin,
    )?)?;
    println!(
        "Remote user service {}.",
        if install {
            "installed; start requested"
        } else {
            "stopped and removed"
        }
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn qualification_origin_is_preserved_by_every_service_renderer() {
        let endpoint = Some("https://station.example.ts.net");
        let origin = Some("https://grok.fabrials.com");
        let program = Path::new("/bin/spanreed");
        assert_eq!(
            command_args(endpoint, origin).unwrap(),
            [
                "remote",
                "private-serve",
                "https://station.example.ts.net",
                "--web-origin",
                "https://grok.fabrials.com"
            ]
        );
        assert!(
            systemd(program, endpoint, origin)
                .unwrap()
                .contains("--web-origin https://grok.fabrials.com")
        );
        assert!(
            launch_agent(program, endpoint, origin).unwrap().contains(
                "<string>--web-origin</string><string>https://grok.fabrials.com</string>"
            )
        );
        assert!(
            windows_task(program, endpoint, origin)
                .unwrap()
                .contains("--web-origin https://grok.fabrials.com'")
        );
        assert!(
            !systemd(program, endpoint, None)
                .unwrap()
                .contains("--web-origin")
        );
        assert!(command_args(None, origin).is_err());
        for invalid in [
            "https://grok.fabrials.com/",
            "https://grok.fabrials.com:443",
            "https://grok.fabrials.com;id",
            "https://other.example",
        ] {
            assert!(systemd(program, endpoint, Some(invalid)).is_err());
            assert!(launch_agent(program, endpoint, Some(invalid)).is_err());
            assert!(windows_task(program, endpoint, Some(invalid)).is_err());
        }
    }

    #[test]
    fn service_flags_allow_either_order_and_reject_duplicates_or_missing_values() {
        let first = [
            "print",
            "--private-origin",
            "https://station.example.ts.net",
            "--web-origin",
            "https://grok.fabrials.com",
        ]
        .map(String::from);
        let second = [
            "print",
            "--web-origin",
            "https://grok.fabrials.com",
            "--private-origin",
            "https://station.example.ts.net",
        ]
        .map(String::from);
        assert_eq!(parse_args(&first).unwrap(), parse_args(&second).unwrap());
        for args in [
            vec!["print", "--web-origin"],
            vec![
                "print",
                "--web-origin",
                "https://grok.fabrials.com",
                "--web-origin",
                "https://desktop.grok.me",
            ],
            vec![
                "install",
                "--private-origin",
                "https://station.example.ts.net",
                "--private-origin",
                "https://other.example",
            ],
            vec!["uninstall", "--web-origin", "https://grok.fabrials.com"],
        ] {
            assert!(parse_args(&args.into_iter().map(String::from).collect::<Vec<_>>()).is_err());
        }
    }
    #[test]
    fn service_paths_are_escaped_and_user_scoped() {
        let path = Path::new("/a b/%f/$USER/'x&\".exe");
        let linux = systemd(path, None, None).unwrap();
        assert!(linux.contains("%%f"));
        assert!(linux.contains("$$USER"));
        assert!(linux.contains("Restart=on-failure"));
        assert!(linux.contains("UMask=0077"));
        assert!(
            launch_agent(path, None, None)
                .unwrap()
                .contains("&amp;&quot;")
        );
        let windows = windows_task(path, None, None).unwrap();
        assert!(windows.contains("-LogonType Interactive -RunLevel Limited"));
        assert!(windows.contains("''x"));
        assert!(!windows.contains("SYSTEM"));
        assert!(windows.contains("-ExecutionTimeLimit ([TimeSpan]::Zero)"));
        assert!(windows.contains("-RestartCount 10"));
        assert!(windows.contains("-Settings $remoteSettings"));
        assert!(systemd(Path::new("/a\nb"), None, None).is_err());
    }
    #[test]
    fn private_origin_is_validated_before_rendering_service_commands() {
        let program = Path::new("/bin/spanreed");
        let origin = Some("https://station.example.ts.net");
        assert!(
            systemd(program, origin, None)
                .unwrap()
                .contains("remote private-serve https://station.example.ts.net")
        );
        assert!(launch_agent(program, origin, None).unwrap().contains("<string>remote</string><string>private-serve</string><string>https://station.example.ts.net</string>"));
        assert!(
            windows_task(program, origin, None)
                .unwrap()
                .contains("-Argument 'remote private-serve https://station.example.ts.net'")
        );
        assert!(
            systemd(program, None, None)
                .unwrap()
                .contains("remote serve")
        );
        for invalid in [
            "https://station.example.ts.net/",
            "http://station.example.ts.net",
            "https://station.example.ts.net/#secret",
            "https://station.example.ts.net@attacker.test",
            "https://station.example.ts.net;id",
            "https://bad\'.ts.net",
            "https://bad&.ts.net",
            "https://station.example.ts.net\n",
        ] {
            assert!(systemd(program, Some(invalid), None).is_err(), "{invalid}");
            assert!(
                launch_agent(program, Some(invalid), None).is_err(),
                "{invalid}"
            );
            assert!(
                windows_task(program, Some(invalid), None).is_err(),
                "{invalid}"
            );
        }
    }

    #[test]
    fn linux_actions_never_use_system_units_or_shell_interpolation() {
        let steps = plan(
            Platform::Linux,
            true,
            Path::new("/home/test/.config"),
            Path::new("/home/test/a b/spanreed-remote"),
            None,
            None,
            None,
        )
        .unwrap();
        assert!(
            matches!(&steps[0], Step::Write(path, _) if path == Path::new("/home/test/.config/systemd/user/spanreed-remote.service"))
        );
        for step in &steps[1..] {
            assert!(
                matches!(step, Step::Run(command,args) if command == "systemctl" && args.first().unwrap() == "--user")
            );
        }
        let remove = plan(
            Platform::Linux,
            false,
            Path::new("/home/test/.config"),
            Path::new("/bin/remote"),
            None,
            None,
            None,
        )
        .unwrap();
        assert!(matches!(&remove[0], Step::Run(_,args) if args.contains(&"--now".to_owned())));
        assert!(matches!(&remove[1], Step::Remove(_)));
    }
    #[test]
    fn mac_targets_only_current_gui_domain_and_windows_only_current_sid() {
        let steps = plan(
            Platform::Mac,
            true,
            Path::new("/Users/test"),
            Path::new("/Applications/remote"),
            Some(501),
            None,
            None,
        )
        .unwrap();
        assert!(matches!(&steps[1], Step::Run(_,args) if args[1] == "gui/501"));
        let uninstall = plan(
            Platform::Mac,
            false,
            Path::new("/Users/test"),
            Path::new("/Applications/remote"),
            Some(501),
            None,
            None,
        )
        .unwrap();
        assert!(
            matches!(&uninstall[0], Step::Run(_,args) if args[1] == "gui/501/com.fabrials.spanreed.remote")
        );
        assert!(windows_remove().contains("GetCurrent().User.Value"));
        assert!(windows_remove().contains("Unregister-ScheduledTask -TaskName $remoteName"));
    }
}
