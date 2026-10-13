use fabrials_agent_host::control::{self, ControlRequest, ControlResponse};
use std::path::{Path, PathBuf};

const USAGE: &str = "usage: spanreed remote roots list|authorize <path>|revoke <root-id>";

fn request(args: &[String]) -> Result<ControlRequest, String> {
    match args {
        [action] if action == "list" => Ok(ControlRequest::ListRemoteRoots),
        [action, path] if action == "authorize" && !path.is_empty() => {
            let path = PathBuf::from(path);
            let path = if path.is_absolute() {
                path
            } else {
                std::env::current_dir()
                    .map_err(|_| "current_directory_unavailable")?
                    .join(path)
            };
            Ok(ControlRequest::AuthorizeRemoteRoot { path })
        }
        [action, root_id] if action == "revoke" && !root_id.is_empty() => {
            Ok(ControlRequest::RevokeRemoteRoot {
                root_id: root_id.clone(),
            })
        }
        _ => Err(USAGE.into()),
    }
}

pub async fn run(directory: &Path, args: &[String]) -> Result<(), String> {
    let request = request(args)?;
    let response = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        control::call(directory, &request),
    )
    .await
    .map_err(|_| "remote_host_timeout")?
    .map_err(|_| "remote_host_not_running")?;
    match response {
        ControlResponse::RemoteRoots { roots } => {
            println!(
                "{}",
                serde_json::to_string(&roots).map_err(|_| "roots_serialization_failed")?
            );
            Ok(())
        }
        ControlResponse::Error { code } => Err(code),
        _ => Err("remote_roots_unavailable".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    #[test]
    fn root_authorization_is_local_and_resolves_relative_paths_at_the_caller() {
        let ControlRequest::AuthorizeRemoteRoot { path } =
            request(&args(&["authorize", "my-projects"])).unwrap()
        else {
            panic!("wrong request");
        };
        assert_eq!(path, std::env::current_dir().unwrap().join("my-projects"));
    }

    #[test]
    fn root_commands_reject_missing_and_extra_arguments() {
        for values in [
            &[][..],
            &["authorize"][..],
            &["authorize", ""][..],
            &["list", "extra"][..],
            &["revoke"][..],
            &["revoke", ""][..],
        ] {
            assert!(request(&args(values)).is_err());
        }
        assert!(matches!(
            request(&args(&["list"])),
            Ok(ControlRequest::ListRemoteRoots)
        ));
        assert!(
            matches!(request(&args(&["revoke", "opaque-root"])), Ok(ControlRequest::RevokeRemoteRoot { root_id }) if root_id == "opaque-root")
        );
    }
}
