//! Discovery of the local Antigravity language server (process + ports).

use crate::proc;

pub(crate) struct Discovered {
    pub(crate) csrf: String,
    pub(crate) ports: Vec<u16>,
}

pub(crate) fn discover() -> Option<Discovered> {
    // language_server process carrying an antigravity marker.
    let procs = proc::find_processes(&["language_server", "antigravity"]);
    for p in procs {
        let csrf = proc::extract_flag(&p.cmdline, "--csrf_token");
        let mut ports = proc::listening_ports(p.pid);
        // Prefer the explicit extension server port if advertised.
        if let Some(port) = proc::extract_flag(&p.cmdline, "--extension_server_port")
            .and_then(|v| v.parse::<u16>().ok())
            && !ports.contains(&port)
        {
            ports.insert(0, port);
        }
        if let Some(csrf) = csrf
            && !ports.is_empty()
        {
            return Some(Discovered { csrf, ports });
        }
    }
    None
}
