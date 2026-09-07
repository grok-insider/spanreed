//! Deprecated CLI shim: `spanreed grok` → `spanreed account`.

use super::protocol::{AddonCaps, AddonHello};
use super::Addon;
use crate::model::ProviderOutput;

pub struct GrokBridge;

impl Addon for GrokBridge {
    fn hello(&self) -> AddonHello {
        AddonHello {
            id: "grok-bridge".into(),
            name: "Grok identity driver".into(),
            caps: AddonCaps {
                providers: vec![],
                commands: vec!["grok".into()],
                listeners: true,
            },
        }
    }

    fn detect(&self, _provider_id: &str) -> bool {
        false
    }

    fn probe(&self, provider_id: &str) -> ProviderOutput {
        ProviderOutput::error(provider_id, "Grok", "use host accounts (spanreed account)")
    }

    fn command(&self, argv: &[String]) -> (String, String, i32) {
        let rest = if argv.first().map(String::as_str) == Some("grok") {
            &argv[1..]
        } else {
            argv
        };
        match forward(rest) {
            Ok(out) => (out, String::new(), 0),
            Err(e) => (String::new(), e, 1),
        }
    }
}

fn forward(args: &[String]) -> Result<String, String> {
    let sub = args.first().map(String::as_str).unwrap_or("list");
    let note = "note: `spanreed grok` is deprecated; use `spanreed account`\n";
    let mapped = match sub {
        "list" | "ls" => vec!["ls".into()],
        "import" => {
            let id = args
                .get(1)
                .ok_or("usage: spanreed account import grok --name <id>")?;
            vec!["import".into(), "grok".into(), "--name".into(), id.clone()]
        }
        "add" => {
            let id = args
                .get(1)
                .ok_or("usage: spanreed account add grok --name <id>")?;
            vec!["add".into(), "grok".into(), "--name".into(), id.clone()]
        }
        "use" => {
            let id = args.get(1).ok_or("usage: spanreed account use grok/<id>")?;
            vec!["use".into(), format!("grok/{id}")]
        }
        "login" => {
            let id = args
                .get(1)
                .ok_or("usage: spanreed account login grok/<id>")?;
            vec!["login".into(), format!("grok/{id}")]
        }
        "rm" => {
            let id = args.get(1).ok_or("usage: spanreed account rm grok/<id>")?;
            vec!["rm".into(), format!("grok/{id}")]
        }
        "serve" => {
            return Ok("the capture service is the fabric (127.0.0.1:18736).\n\
                 active account is injected there; parallel: /acct/<alias>/v1\n\
                 start: spanreed capture serve\n"
                .into());
        }
        "usage" | "help" | "-h" | "--help" => {
            return Ok(format!("{note}{}", crate::drivers::account_help()));
        }
        other => {
            return Err(format!(
                "unknown grok subcommand: {other}\n{note}{}",
                crate::drivers::account_help()
            ));
        }
    };
    crate::drivers::dispatch_account(&mapped).map(|s| format!("{note}{s}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::addons::Addon;

    #[test]
    fn protocol_hello() {
        let h = GrokBridge.hello();
        assert_eq!(h.id, "grok-bridge");
        assert!(h.caps.commands.contains(&"grok".into()));
    }
}
