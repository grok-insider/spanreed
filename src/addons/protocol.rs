//! Stable JSON contract for in-process and out-of-process addons.

use serde::{Deserialize, Serialize};

use crate::model::ProviderOutput;

pub const PROTOCOL_V: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AddonHello {
    pub id: String,
    pub name: String,
    pub caps: AddonCaps,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AddonCaps {
    #[serde(default)]
    pub providers: Vec<String>,
    #[serde(default)]
    pub commands: Vec<String>,
    #[serde(default)]
    pub listeners: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AddonRequest {
    pub v: u32,
    pub op: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub argv: Option<Vec<String>>,
}

impl AddonRequest {
    #[allow(dead_code)]
    pub fn hello() -> Self {
        Self {
            v: PROTOCOL_V,
            op: "hello".into(),
            provider: None,
            argv: None,
        }
    }

    #[allow(dead_code)]
    pub fn detect(provider: impl Into<String>) -> Self {
        Self {
            v: PROTOCOL_V,
            op: "detect".into(),
            provider: Some(provider.into()),
            argv: None,
        }
    }

    #[allow(dead_code)]
    pub fn probe(provider: impl Into<String>) -> Self {
        Self {
            v: PROTOCOL_V,
            op: "probe".into(),
            provider: Some(provider.into()),
            argv: None,
        }
    }

    pub fn command(argv: Vec<String>) -> Self {
        Self {
            v: PROTOCOL_V,
            op: "command".into(),
            provider: None,
            argv: Some(argv),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AddonResponse {
    pub v: u32,
    pub ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hello: Option<AddonHello>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detected: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output: Option<ProviderOutput>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stdout: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stderr: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code: Option<i32>,
}

#[allow(dead_code)]
impl AddonResponse {
    pub fn hello(hello: AddonHello) -> Self {
        Self {
            v: PROTOCOL_V,
            ok: true,
            error: None,
            hello: Some(hello),
            detected: None,
            output: None,
            stdout: None,
            stderr: None,
            code: None,
        }
    }

    pub fn err(msg: impl Into<String>) -> Self {
        Self {
            v: PROTOCOL_V,
            ok: false,
            error: Some(msg.into()),
            hello: None,
            detected: None,
            output: None,
            stdout: None,
            stderr: None,
            code: None,
        }
    }

    pub fn detect(detected: bool) -> Self {
        Self {
            v: PROTOCOL_V,
            ok: true,
            error: None,
            hello: None,
            detected: Some(detected),
            output: None,
            stdout: None,
            stderr: None,
            code: None,
        }
    }

    pub fn probe(output: ProviderOutput) -> Self {
        Self {
            v: PROTOCOL_V,
            ok: true,
            error: None,
            hello: None,
            detected: None,
            output: Some(output),
            stdout: None,
            stderr: None,
            code: None,
        }
    }

    pub fn command(stdout: String, stderr: String, code: i32) -> Self {
        Self {
            v: PROTOCOL_V,
            ok: code == 0,
            error: None,
            hello: None,
            detected: None,
            output: None,
            stdout: Some(stdout),
            stderr: Some(stderr),
            code: Some(code),
        }
    }
}

/// Dispatch one request against an in-process [`crate::addons::Addon`].
#[allow(dead_code)]
pub fn dispatch_inproc(addon: &dyn crate::addons::Addon, req: &AddonRequest) -> AddonResponse {
    if req.v != PROTOCOL_V {
        return AddonResponse::err(format!("unsupported protocol v{}", req.v));
    }
    match req.op.as_str() {
        "hello" => AddonResponse::hello(addon.hello()),
        "detect" => {
            let id = req.provider.as_deref().unwrap_or("");
            AddonResponse::detect(addon.detect(id))
        }
        "probe" => {
            let id = req.provider.as_deref().unwrap_or("");
            AddonResponse::probe(addon.probe(id))
        }
        "command" => {
            let argv = req.argv.clone().unwrap_or_default();
            let (stdout, stderr, code) = addon.command(&argv);
            AddonResponse::command(stdout, stderr, code)
        }
        "shutdown" => AddonResponse {
            v: PROTOCOL_V,
            ok: true,
            error: None,
            hello: None,
            detected: None,
            output: None,
            stdout: None,
            stderr: None,
            code: None,
        },
        other => AddonResponse::err(format!("unknown op: {other}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::addons::grok_bridge::GrokBridge;
    use crate::addons::Addon;

    #[test]
    fn hello_roundtrip() {
        let resp = dispatch_inproc(&GrokBridge, &AddonRequest::hello());
        assert!(resp.ok);
        let h = resp.hello.expect("hello");
        assert_eq!(h.id, GrokBridge.hello().id);
        assert!(h.caps.commands.iter().any(|c| c == "grok"));
    }

    #[test]
    fn unknown_op() {
        let resp = dispatch_inproc(
            &GrokBridge,
            &AddonRequest {
                v: 1,
                op: "nope".into(),
                provider: None,
                argv: None,
            },
        );
        assert!(!resp.ok);
    }
}
