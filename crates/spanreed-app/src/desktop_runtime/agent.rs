//! The agent host (`spanreed agent serve`, the absorbed grok-bridge) run
//! inside this GUI process: its own thread and Tokio runtime, and its own
//! loopback port from the agent state, never the capture listener's.

use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use fabrials_agent_host::server::{DEFAULT_AGENT_PROGRAM, HostState};
use fabrials_agent_host::{CliDefaults, HostConfig, HostIdentity};

use super::ProxyState;

/// How `/healthz` and the CLI name this host.
pub const HOST: HostIdentity = HostIdentity::new("spanreed", env!("CARGO_PKG_VERSION"));

/// The command users type to reach the agent host.
pub const COMMAND: &str = "spanreed agent";

/// How long `start` waits for the host to listen before reporting "starting".
const READY_TIMEOUT: Duration = Duration::from_secs(10);

pub fn defaults() -> CliDefaults {
    CliDefaults::new(HOST, COMMAND)
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentStatus {
    pub state: ProxyState,
    /// Loopback origin once listening, e.g. `http://<id>.grok-light.localhost:23456`.
    pub origin: Option<String>,
    pub port: Option<u16>,
    pub error: Option<String>,
}

/// What the embedded host runs.
#[derive(Clone, Debug)]
pub struct AgentLaunch {
    pub config: HostConfig,
}

impl AgentLaunch {
    /// The state directory, agent program and origins `spanreed agent serve` uses.
    pub fn from_env() -> Result<Self, String> {
        let dirs = fabrials_agent_host::state_dir::resolve_state_dirs()
            .map_err(|error| error.to_string())?;
        let defaults = defaults();
        let mut config = HostConfig::new(dirs.state_dir, defaults.identity);
        config.legacy_state_dir = dirs.legacy_state_dir;
        config.agent_program = agent_program();
        config.allowed_origins = defaults.allowed_origins;
        Ok(Self { config })
    }

    pub fn state_dir(&self) -> &PathBuf {
        &self.config.state_dir
    }
}

/// `FABRIALS_AGENT_PROGRAM`, then `GROK_BRIDGE_AGENT`, then `grok`.
fn agent_program() -> String {
    [
        fabrials_agent_host::cli::AGENT_PROGRAM_ENV,
        fabrials_agent_host::cli::LEGACY_AGENT_PROGRAM_ENV,
    ]
    .into_iter()
    .find_map(|name| std::env::var(name).ok().filter(|value| !value.is_empty()))
    .unwrap_or_else(|| DEFAULT_AGENT_PROGRAM.to_owned())
}

/// The capture fabric's port; the agent host must never take it.
fn capture_port() -> Option<u16> {
    crate::capture::DEFAULT_GROK_CLI_BIND
        .rsplit(':')
        .next()
        .and_then(|port| port.parse().ok())
}

fn check_port(port: u16) -> Result<(), String> {
    if Some(port) == capture_port() {
        return Err(format!(
            "The agent host port {port} is the capture listener's; run `spanreed agent repair`"
        ));
    }
    Ok(())
}

type Ready = Result<Listening, String>;

struct Listening {
    host: Arc<HostState>,
    origin: String,
    port: u16,
}

struct Running {
    host: Arc<HostState>,
    origin: String,
    port: u16,
    thread: JoinHandle<Result<(), String>>,
}

/// Started, not yet listening.
struct Starting {
    ready: Receiver<Ready>,
    thread: JoinHandle<Result<(), String>>,
}

#[derive(Default)]
struct Controller {
    starting: Option<Starting>,
    running: Option<Running>,
    error: Option<String>,
}

impl Controller {
    fn status(&mut self) -> AgentStatus {
        self.promote();
        if self
            .running
            .as_ref()
            .is_some_and(|running| running.thread.is_finished())
            && let Some(running) = self.running.take()
            && let Err(error) = join(running.thread)
        {
            self.error = Some(error);
        }
        let (state, origin, port) = match (&self.starting, &self.running) {
            (Some(_), _) => (ProxyState::Running, None, None),
            (None, Some(running)) if running.host.is_shutting_down() => (
                ProxyState::Stopping,
                Some(running.origin.clone()),
                Some(running.port),
            ),
            (None, Some(running)) => (
                ProxyState::Running,
                Some(running.origin.clone()),
                Some(running.port),
            ),
            (None, None) => (ProxyState::Stopped, None, None),
        };
        AgentStatus {
            state,
            origin,
            port,
            error: self.error.clone(),
        }
    }

    /// Move a finished start into `running` or `error`.
    fn promote(&mut self) {
        let Some(starting) = self.starting.take() else {
            return;
        };
        match starting.ready.try_recv() {
            Ok(Ok(listening)) => self.listening(listening, starting.thread),
            Ok(Err(error)) => {
                let _ = join(starting.thread);
                self.error = Some(error);
            }
            Err(TryRecvError::Empty) => self.starting = Some(starting),
            Err(TryRecvError::Disconnected) => {
                self.error = Some(
                    join(starting.thread)
                        .err()
                        .unwrap_or_else(|| "Agent host stopped".into()),
                );
            }
        }
    }

    fn listening(&mut self, listening: Listening, thread: JoinHandle<Result<(), String>>) {
        self.error = None;
        self.running = Some(Running {
            host: listening.host,
            origin: listening.origin,
            port: listening.port,
            thread,
        });
    }

    fn start(&mut self, launch: AgentLaunch) -> Result<AgentStatus, String> {
        self.status();
        if self.starting.is_some() || self.running.is_some() {
            return Err("This process already runs the agent host".into());
        }
        let (ready_tx, ready) = mpsc::sync_channel(1);
        let thread = std::thread::Builder::new()
            .name("spanreed-agent-host".into())
            .spawn(move || serve(launch, ready_tx))
            .map_err(|_| "Could not start the agent host thread")?;
        match ready.recv_timeout(READY_TIMEOUT) {
            Ok(Ok(listening)) => {
                self.listening(listening, thread);
                Ok(self.status())
            }
            Ok(Err(error)) => {
                let _ = join(thread);
                self.error = Some(error.clone());
                Err(error)
            }
            Err(_) => {
                self.starting = Some(Starting { ready, thread });
                Ok(self.status())
            }
        }
    }

    fn stop(&mut self) -> AgentStatus {
        self.promote();
        if let Some(running) = &self.running {
            running.host.request_shutdown();
        }
        self.status()
    }
}

impl Drop for Controller {
    fn drop(&mut self) {
        self.promote();
        let Some(running) = self.running.take() else {
            return;
        };
        running.host.request_shutdown();
        // Give the host time to stop its agent; the process may be exiting.
        let deadline = Instant::now() + Duration::from_secs(5);
        while !running.thread.is_finished() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}

fn join(thread: JoinHandle<Result<(), String>>) -> Result<(), String> {
    thread
        .join()
        .unwrap_or_else(|_| Err("Agent host stopped unexpectedly".into()))
}

/// The host thread: its own runtime, `start`, report readiness, then serve
/// until shutdown.
fn serve(launch: AgentLaunch, ready: mpsc::SyncSender<Ready>) -> Result<(), String> {
    let fail = |ready: &mpsc::SyncSender<Ready>, error: String| {
        let _ = ready.try_send(Err(error.clone()));
        Err(error)
    };
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .thread_name("spanreed-agent-rt")
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(error) => return fail(&ready, format!("Agent host runtime: {error}")),
    };
    runtime.block_on(async move {
        let running = match fabrials_agent_host::start(launch.config).await {
            Ok(running) => running,
            Err(error) => return fail(&ready, error.to_string()),
        };
        let origin = running.origin().clone();
        if let Err(error) = check_port(origin.port()) {
            running.shutdown();
            let _ = running.wait().await;
            return fail(&ready, error);
        }
        let _ = ready.try_send(Ok(Listening {
            host: Arc::clone(running.state()),
            origin: origin.to_string(),
            port: origin.port(),
        }));
        running.wait().await.map_err(|error| error.to_string())
    })
}

/// The agent host this process owns. Owned by `AppContext`; dropping it
/// stops the host.
pub struct AgentControl {
    controller: Mutex<Controller>,
    launch: Box<dyn Fn() -> Result<AgentLaunch, String> + Send + Sync>,
}

impl Default for AgentControl {
    fn default() -> Self {
        Self {
            controller: Mutex::default(),
            launch: Box::new(AgentLaunch::from_env),
        }
    }
}

impl AgentControl {
    /// A controller that always runs `launch` (tests and custom hosts).
    pub fn with_launch(launch: AgentLaunch) -> Self {
        Self {
            controller: Mutex::default(),
            launch: Box::new(move || Ok(launch.clone())),
        }
    }

    fn controller(&self) -> Result<MutexGuard<'_, Controller>, String> {
        self.controller
            .lock()
            .map_err(|_| "Agent host control unavailable".into())
    }

    pub fn status(&self) -> Result<AgentStatus, String> {
        Ok(self.controller()?.status())
    }

    pub fn start(&self) -> Result<AgentStatus, String> {
        let launch = (self.launch)()?;
        self.controller()?.start(launch)
    }

    pub fn stop(&self) -> Result<AgentStatus, String> {
        Ok(self.controller()?.stop())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::path::Path;

    /// The fabrials-agent-host scripted ACP agent, built on demand into this
    /// test's profile and target. `None` when it cannot be built (the Nix
    /// check phase has no registry for the example's dev-dependencies).
    fn fake_agent() -> Option<PathBuf> {
        let exe = std::env::current_exe().ok()?;
        let profile_dir = exe.parent().and_then(Path::parent)?;
        let path = profile_dir
            .join("examples")
            .join(format!("fake_agent{}", std::env::consts::EXE_SUFFIX));
        if path.exists() {
            return Some(path);
        }
        let mut command = std::process::Command::new(env!("CARGO"));
        command.args([
            "build",
            "--offline",
            "-p",
            "fabrials-agent-host",
            "--example",
            "fake_agent",
        ]);
        let profile = profile_dir.file_name().and_then(|name| name.to_str());
        if let Some(profile) = profile.filter(|profile| *profile != "debug") {
            command.args(["--profile", profile]);
        }
        let triple = profile_dir
            .parent()
            .and_then(Path::file_name)
            .and_then(|name| name.to_str())
            .filter(|name| name.matches('-').count() >= 2);
        if let Some(triple) = triple {
            command.args(["--target", triple]);
        }
        let built = command.status().is_ok_and(|status| status.success());
        (built && path.exists()).then_some(path)
    }

    /// The fake agent, or a missing program: the host serves and keeps
    /// relaunching either way, which is all the controller relies on.
    fn agent_program() -> String {
        match fake_agent() {
            Some(path) => path.to_string_lossy().into_owned(),
            None => {
                eprintln!("fake_agent unavailable; testing the host with a missing agent");
                std::env::temp_dir()
                    .join("spanreed-missing-agent")
                    .to_string_lossy()
                    .into_owned()
            }
        }
    }

    /// A free port in the agent host's allocation range.
    fn free_port() -> u16 {
        static NEXT: std::sync::atomic::AtomicU16 = std::sync::atomic::AtomicU16::new(24_300);
        loop {
            let port = NEXT.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            assert!(port < 25_000, "exhausted the test port range");
            if std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, port)).is_ok() {
                return port;
            }
        }
    }

    struct StateDir(PathBuf);

    impl Drop for StateDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn state_dir(port: u16) -> StateDir {
        let root =
            std::env::temp_dir().join(format!("spanreed-agent-host-{}-{port}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700)).unwrap();
        }
        fabrials_agent_host::state::persist(
            &root,
            &fabrials_agent_host::state::InstallIdentity {
                install_id: "0123456789abcdef0123456789abcdef".into(),
                port,
            },
        )
        .unwrap();
        StateDir(root)
    }

    fn launch(dir: &Path) -> AgentLaunch {
        let mut config = HostConfig::new(dir, HOST);
        config.agent_program = agent_program();
        config.picker = Some(Arc::new(
            fabrials_agent_host::picker::UnavailableDirectoryPicker,
        ));
        AgentLaunch { config }
    }

    fn health(port: u16) -> String {
        let mut stream = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        write!(
            stream,
            "GET /healthz HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n"
        )
        .unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).unwrap();
        response
    }

    fn stopped(control: &AgentControl) -> AgentStatus {
        let until = Instant::now() + Duration::from_secs(10);
        loop {
            let status = control.status().unwrap();
            if status.state == ProxyState::Stopped {
                return status;
            }
            assert!(
                Instant::now() < until,
                "agent host did not stop: {status:?}"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    #[test]
    fn starts_on_its_own_port_serves_health_stops_and_restarts() {
        let port = free_port();
        let dir = state_dir(port);
        let control = AgentControl::with_launch(launch(&dir.0));
        assert_eq!(control.status().unwrap().state, ProxyState::Stopped);
        for _ in 0..2 {
            let status = control.start().unwrap();
            assert_eq!(status.state, ProxyState::Running, "{status:?}");
            assert_eq!(status.port, Some(port));
            assert_ne!(status.port, capture_port());
            assert!(status.origin.unwrap().ends_with(&format!(":{port}")));
            assert!(control.start().is_err(), "a second start must be refused");
            let response = health(port);
            assert!(response.starts_with("HTTP/1.1 200"), "{response}");
            assert!(response.contains("spanreed"), "{response}");
            assert_eq!(control.stop().unwrap().port, Some(port));
            let status = stopped(&control);
            assert_eq!(status.error, None);
        }
    }

    #[test]
    fn a_second_owner_of_the_state_directory_reports_the_error() {
        let port = free_port();
        let dir = state_dir(port);
        let first = AgentControl::with_launch(launch(&dir.0));
        let second = AgentControl::with_launch(launch(&dir.0));
        first.start().unwrap();
        assert!(second.start().is_err());
        let status = second.status().unwrap();
        assert_eq!(status.state, ProxyState::Stopped);
        assert!(status.error.is_some());
        first.stop().unwrap();
        stopped(&first);
    }

    #[test]
    fn the_capture_port_is_refused() {
        let capture = capture_port().expect("capture bind has a port");
        assert_eq!(capture, 18736);
        assert!(check_port(capture).is_err());
        assert!(check_port(23_456).is_ok());
    }
}
