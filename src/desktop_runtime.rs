//! Lifecycle of the proxy instance owned by this GUI process.
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex, OnceLock,
};
use std::thread::JoinHandle;
use std::time::Duration;

#[cfg_attr(feature = "contracts", derive(ts_rs::TS))]
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ProxyState {
    Running,
    Stopping,
    Stopped,
}

#[cfg_attr(feature = "contracts", derive(ts_rs::TS))]
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub state: ProxyState,
    pub bind: String,
    pub error: Option<String>,
}
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
            bind: crate::grok_proxy::DEFAULT_GROK_CLI_BIND.into(),
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
                let result = fabrials_runtime::listener::serve_embedded(
                    Arc::new(host),
                    worker_stop,
                    64,
                    128 * 1024 * 1024,
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
fn controller() -> &'static Mutex<Controller> {
    static CONTROLLER: OnceLock<Mutex<Controller>> = OnceLock::new();
    CONTROLLER.get_or_init(|| Mutex::new(Controller::default()))
}
pub fn status() -> Result<Status, String> {
    Ok(controller()
        .lock()
        .map_err(|_| "Proxy control unavailable")?
        .status())
}
pub fn start(bind: &str) -> Result<Status, String> {
    controller()
        .lock()
        .map_err(|_| "Proxy control unavailable")?
        .start(bind)
}
pub fn stop() -> Result<Status, String> {
    Ok(controller()
        .lock()
        .map_err(|_| "Proxy control unavailable")?
        .stop())
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
        for _ in 0..2 {
            assert_eq!(
                controller.start(&address).unwrap().state,
                ProxyState::Running
            );
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
