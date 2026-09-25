//! CodexBar providers whose quota path does not fit Spanreed.
//!
//! Each entry stays in `spanreed list` and explains the missing credential or
//! API when force-probed. Browser cookie import, paid validation calls, and
//! cloud signing clients are intentionally not added here.

use crate::model::ProviderOutput;
use crate::providers::Provider;

struct Note {
    id: &'static str,
    name: &'static str,
    reason: &'static str,
}

const NOTES: &[Note] = &[
    Note {
        id: "azureopenai",
        name: "Azure OpenAI",
        reason: "Azure OpenAI exposes no usage API. Its CodexBar check is a paid chat completion, which Spanreed will not send on a refresh loop.",
    },
    Note {
        id: "opencode",
        name: "OpenCode",
        reason: "OpenCode workspace usage is a browser-cookie dashboard. Local OpenCode Go history is the opencode-go provider.",
    },
    Note {
        id: "alibaba",
        name: "Alibaba Coding Plan",
        reason: "Alibaba Coding Plan quota uses Aliyun console cookies, a CSRF token, and form posts. An API key does not return that document.",
    },
    Note {
        id: "alibabatokenplan",
        name: "Alibaba Token Plan",
        reason: "Alibaba Token Plan quota is a Bailian console cookie API with the same CSRF and form-post constraints.",
    },
    Note {
        id: "qwencloud",
        name: "Qwen Cloud",
        reason: "Qwen Cloud token-plan windows are read from console cookies, not an API key.",
    },
    Note {
        id: "vertexai",
        name: "Vertex AI",
        reason: "Vertex quota comes from Cloud Monitoring with gcloud application-default credentials. Spanreed does not ship that client.",
    },
    Note {
        id: "augment",
        name: "Augment",
        reason: "Augment credits come from the auggie CLI or browser cookies. There is no documented API-key quota call.",
    },
    Note {
        id: "t3chat",
        name: "T3 Chat",
        reason: "T3 Chat usage is a tRPC call authenticated only by browser cookies.",
    },
    Note {
        id: "windsurf",
        name: "Windsurf",
        reason: "Windsurf plan status is a protobuf call that uses a Chromium localStorage session.",
    },
    Note {
        id: "zed",
        name: "Zed",
        reason: "Zed plan data is read from the editor keychain session. Spanreed does not prompt another app's secret.",
    },
    Note {
        id: "mimo",
        name: "Xiaomi MiMo",
        reason: "Xiaomi MiMo balance is a browser-cookie console API.",
    },
    Note {
        id: "doubao",
        name: "Doubao",
        reason: "Doubao's CodexBar check is a paid Ark chat completion plus a signed Volcengine plan call. Spanreed will not spend a completion to check a key.",
    },
    Note {
        id: "sakana",
        name: "Sakana",
        reason: "Sakana quota is parsed from billing HTML with a manual cookie. There is no JSON quota API.",
    },
    Note {
        id: "abacus",
        name: "Abacus",
        reason: "Abacus compute points require a browser session cookie.",
    },
    Note {
        id: "mistral",
        name: "Mistral",
        reason: "Mistral console billing requires browser cookies and a CSRF token.",
    },
    Note {
        id: "commandcode",
        name: "Command Code",
        reason: "Command Code billing is a session-cookie API with no API-key path.",
    },
    Note {
        id: "qoder",
        name: "Qoder",
        reason: "Qoder credits require a regional browser session.",
    },
    Note {
        id: "stepfun",
        name: "StepFun",
        reason: "StepFun quota uses an Oasis session minted by password login. Spanreed does not collect passwords.",
    },
    Note {
        id: "bedrock",
        name: "AWS Bedrock",
        reason: "Bedrock spend uses SigV4 against Cost Explorer. Spanreed does not ship an AWS signer.",
    },
    Note {
        id: "helmcode",
        name: "Helmcode",
        reason: "Helmcode quotas use tenant cookies on the Helmcode cloud API.",
    },
    Note {
        id: "longcat",
        name: "LongCat",
        reason: "LongCat quota calls are cookie-authenticated platform APIs.",
    },
    Note {
        id: "zoommate",
        name: "ZoomMate",
        reason: "ZoomMate mints a bearer token from a Chrome cookie. Spanreed does not import browser cookies.",
    },
    Note {
        id: "notion",
        name: "Notion AI",
        reason: "Notion AI allowance uses browser cookies and a workspace id from that session.",
    },
    Note {
        id: "replicate",
        name: "Replicate",
        reason: "Replicate billing is read from the account page with browser cookies.",
    },
    Note {
        id: "pi",
        name: "Pi",
        reason: "Pi has no subscription quota. CodexBar prices local transcripts, which would double-count Codex and Claude usage Spanreed already records.",
    },
    Note {
        id: "typesafe",
        name: "TypeSafe",
        reason: "TypeSafe usage is a console page authenticated by browser cookies.",
    },
];

struct SessionOnly {
    note: &'static Note,
}

impl Provider for SessionOnly {
    fn id(&self) -> &'static str {
        self.note.id
    }
    fn name(&self) -> &'static str {
        self.note.name
    }
    fn detect(&self) -> bool {
        false
    }
    fn probe(&self, _ports: crate::ports::ProbePorts<'_>) -> ProviderOutput {
        ProviderOutput::error(self.note.id, self.note.name, self.note.reason)
    }
}

pub(super) fn by_id(id: &str) -> Option<Box<dyn Provider>> {
    NOTES
        .iter()
        .find(|note| note.id == id)
        .map(|note| Box::new(SessionOnly { note }) as Box<dyn Provider>)
}

#[cfg(test)]
pub(super) fn providers() -> Vec<Box<dyn Provider>> {
    NOTES
        .iter()
        .map(|note| Box::new(SessionOnly { note }) as Box<dyn Provider>)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_note_has_an_id_and_a_reason() {
        let mut ids = Vec::new();
        for note in NOTES {
            assert!(!note.id.is_empty());
            assert!(!note.name.is_empty());
            assert!(note.reason.len() > 20, "{}", note.id);
            assert!(!ids.contains(&note.id), "duplicate {}", note.id);
            ids.push(note.id);
            let provider = SessionOnly { note };
            assert!(!provider.detect());
            assert!(provider.probe(crate::ports::ProbePorts::bare()).has_error());
        }
        assert_eq!(providers().len(), NOTES.len());
    }
}
