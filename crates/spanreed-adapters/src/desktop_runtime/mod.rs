//! Lifecycle of the local proxy and the agent host owned by this GUI process.

mod agent;

pub use agent::{AgentControl, AgentLaunch, AgentStatus, defaults as agent_defaults};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
use std::thread::JoinHandle;
use std::time::Duration;

pub use spanreed_app::app::proxy::ProxyState;
pub use spanreed_app::app::proxy::Status;
struct Running {
    stop: Arc<AtomicBool>,
    thread: JoinHandle<Result<(), String>>,
}
struct Controller {
    running: Option<Running>,
    bind: String,
    error: Option<String>,
}
impl Default for Controller {
    fn default() -> Self {
        Self {
            running: None,
            bind: crate::capture::DEFAULT_GROK_CLI_BIND.into(),
            error: None,
        }
    }
}
impl Controller {
    fn status(&mut self) -> Status {
        if self
            .running
            .as_ref()
            .is_some_and(|running| running.thread.is_finished())
        {
            let running = self.running.take().unwrap();
            if let Err(error) = running
                .thread
                .join()
                .unwrap_or_else(|_| Err("Proxy worker stopped unexpectedly".into()))
            {
                self.error = Some(error);
            }
        }
        let state = match &self.running {
            Some(running) if running.stop.load(Ordering::Relaxed) => ProxyState::Stopping,
            Some(_) => ProxyState::Running,
            None => ProxyState::Stopped,
        };
        Status {
            state,
            bind: self.bind.clone(),
            error: self.error.clone(),
        }
    }
    fn start(&mut self, bind: &str) -> Result<Status, String> {
        self.status();
        if self.running.is_some() {
            return Err("This GUI already owns a running or stopping proxy".into());
        }
        let address: std::net::SocketAddr = bind
            .parse()
            .map_err(|_| "Use a numeric loopback address and port")?;
        if !address.ip().is_loopback() || address.port() == 0 {
            return Err("Choose a loopback address with a nonzero port".into());
        }
        let (ready_tx, ready_rx) = std::sync::mpsc::sync_channel(1);
        let ready = ready_tx.clone();
        let host = crate::local_relay::LocalRelay::new(&address.to_string())?.with_ready_handler(
            move |_| {
                let _ = ready.try_send(Ok(()));
            },
        );
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = stop.clone();
        let thread = std::thread::Builder::new()
            .name("spanreed-local-proxy".into())
            .spawn(move || {
                let result = fabrials_fabric::listener::serve_embedded(
                    Arc::new(host),
                    worker_stop,
                    64,
                    crate::local_relay::BODY_MEMORY_LIMIT_BYTES,
                );
                if let Err(error) = &result {
                    let _ = ready_tx.try_send(Err(error.clone()));
                }
                result
            })
            .map_err(|_| "Could not start proxy worker")?;
        self.bind = address.to_string();
        self.error = None;
        self.running = Some(Running {
            stop: stop.clone(),
            thread,
        });
        match ready_rx.recv_timeout(Duration::from_secs(5)) {
            Ok(Ok(())) => Ok(self.status()),
            result => {
                stop.store(true, Ordering::Relaxed);
                let error = match result {
                    Ok(Err(error)) => error,
                    _ => "Proxy startup did not finish in time".into(),
                };
                self.error = Some(error.clone());
                Err(error)
            }
        }
    }
    fn stop(&mut self) -> Status {
        if let Some(running) = &self.running {
            running.stop.store(true, Ordering::Relaxed);
        }
        self.status()
    }
}
/// The local proxy owned by this GUI process. Owned by `AppContext`.
#[derive(Default)]
pub struct ProxyControl {
    controller: Mutex<Controller>,
}

impl ProxyControl {
    fn controller(&self) -> Result<std::sync::MutexGuard<'_, Controller>, String> {
        self.controller
            .lock()
            .map_err(|_| "Proxy control unavailable".into())
    }

    pub fn status(&self) -> Result<Status, String> {
        Ok(self.controller()?.status())
    }

    pub fn start(&self, bind: &str) -> Result<Status, String> {
        self.controller()?.start(bind)
    }

    pub fn stop(&self) -> Result<Status, String> {
        Ok(self.controller()?.stop())
    }
}

impl Drop for Controller {
    fn drop(&mut self) {
        if let Some(running) = &self.running {
            running.stop.store(true, Ordering::Relaxed);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn owns_only_its_listener_and_supports_restart() {
        use std::io::{Read, Write};
        let occupied = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = occupied.local_addr().unwrap().to_string();
        let mut controller = Controller::default();
        assert!(controller.start("0.0.0.0:18736").is_err());
        assert!(controller.start(&address).is_err());
        assert_eq!(occupied.local_addr().unwrap().to_string(), address);
        fn stopped(controller: &mut Controller) {
            let until = std::time::Instant::now() + Duration::from_secs(7);
            while controller.status().state != ProxyState::Stopped {
                assert!(std::time::Instant::now() < until, "proxy did not stop");
                std::thread::sleep(Duration::from_millis(20));
            }
        }
        stopped(&mut controller);
        drop(occupied);
        // Parallel tests also bind ephemeral ports, so a freed port can be taken
        // before the restart; retry on a fresh port only for that reason.
        fn start_on_free_port(controller: &mut Controller, preferred: &str) -> String {
            let mut address = preferred.to_string();
            for _ in 0..20 {
                match controller.start(&address) {
                    Ok(status) => {
                        assert_eq!(status.state, ProxyState::Running);
                        return address;
                    }
                    Err(error) if error.contains("in use") => {
                        let probe = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
                        address = probe.local_addr().unwrap().to_string();
                    }
                    Err(error) => panic!("start failed: {error}"),
                }
            }
            panic!("no free loopback port");
        }
        for _ in 0..2 {
            let address = start_on_free_port(&mut controller, &address);
            assert!(controller.start(&address).is_err());
            let mut stream = std::net::TcpStream::connect(&address).unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(3)))
                .unwrap();
            write!(
                stream,
                "GET /health HTTP/1.1\r\nHost: {address}\r\nConnection: close\r\n\r\n"
            )
            .unwrap();
            let mut response = String::new();
            stream.read_to_string(&mut response).unwrap();
            assert!(response.starts_with("HTTP/1.1 200"), "{response}");
            assert!(response.contains("spanreed"));
            drop(stream);
            controller.stop();
            stopped(&mut controller);
        }
    }
}
