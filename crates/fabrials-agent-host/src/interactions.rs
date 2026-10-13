use crate::protocol::Event;
use serde_json::{Value, json};

#[derive(Debug, Clone)]
pub struct PendingInteraction {
    pub request_id: Value,
    pub session_id: String,
    pub tool_call_id: String,
    pub kind: String,
    pub details: Value,
}

pub fn normalize<'a>(method: &'a str, params: &'a Value) -> (&'a str, &'a Value) {
    let method = method.strip_prefix('_').unwrap_or(method);
    if let (Some(inner_method), Some(inner_params)) = (
        params.get("method").and_then(Value::as_str),
        params.get("params"),
    ) {
        if inner_method.strip_prefix('_').unwrap_or(inner_method) == method {
            return (method, inner_params);
        }
    }
    (method, params)
}

impl PendingInteraction {
    pub fn parse(method: &str, request_id: Value, params: &Value) -> Option<Self> {
        let (method, params) = normalize(method, params);
        if serde_json::to_vec(params).ok()?.len() > 64 * 1024 {
            return None;
        }
        let session_id = params.get("sessionId")?.as_str()?.to_owned();
        let tool_call_id = params.get("toolCallId")?.as_str()?.to_owned();
        if session_id.len() > 256 || tool_call_id.len() > 256 {
            return None;
        }
        let (kind, fields): (&str, &[&str]) = match method {
            "x.ai/ask_user_question" if params.get("questions")?.as_array()?.len() <= 16 => {
                ("question", &["questions", "mode"])
            }
            "x.ai/exit_plan_mode" => ("plan", &["planContent"]),
            "x.ai/mcp/elicit" => (
                "mcp",
                &[
                    "serverName",
                    "message",
                    "mode",
                    "requestedSchema",
                    "url",
                    "elicitationId",
                ],
            ),
            _ => return None,
        };
        let mut details = serde_json::Map::new();
        for field in fields {
            if let Some(value) = params.get(field) {
                details.insert((*field).into(), value.clone());
            }
        }
        Some(Self {
            request_id,
            session_id,
            tool_call_id,
            kind: kind.into(),
            details: Value::Object(details),
        })
    }

    pub fn event(&self, key: &str) -> Event {
        Event::InteractionRequest {
            session_id: self.session_id.clone(),
            request_id: key.into(),
            interaction_kind: self.kind.clone(),
            details: self.details.clone(),
        }
    }

    pub fn answer(&self, answer: &Value) -> Option<Value> {
        let outcome = answer.get("outcome")?.as_str()?;
        if serde_json::to_vec(answer).ok()?.len() > 64 * 1024 {
            return None;
        }
        match (self.kind.as_str(), outcome) {
            ("question", "cancelled")
            | ("plan", "approved" | "abandoned")
            | ("mcp", "cancel" | "decline") => Some(json!({"outcome": outcome})),
            ("plan", "cancelled") => {
                let mut result = json!({"outcome": outcome});
                if let Some(feedback) = answer.get("feedback") {
                    result["feedback"] = json!(feedback.as_str()?);
                }
                Some(result)
            }
            ("question", "accepted") => {
                let answers = answer.get("answers")?.as_object()?;
                let questions = self.details.get("questions")?.as_array()?;
                for (question, selection) in answers {
                    let source = questions.iter().find(|q| {
                        q.get("question").and_then(Value::as_str) == Some(question.as_str())
                    })?;
                    let selected = selection.as_array()?;
                    if selected.is_empty() || selected.len() > 32 {
                        return None;
                    }
                    if source.get("multiSelect").and_then(Value::as_bool) != Some(true)
                        && selected.len() != 1
                    {
                        return None;
                    }
                    for value in selected {
                        let label = value.as_str()?;
                        if label != "Other"
                            && !source.get("options")?.as_array()?.iter().any(|option| {
                                option.get("label").and_then(Value::as_str) == Some(label)
                            })
                        {
                            return None;
                        }
                    }
                }
                let mut result = json!({"outcome": outcome, "answers": answers});
                if let Some(annotations) = answer.get("annotations") {
                    let mut clean = serde_json::Map::new();
                    for (question, annotation) in annotations.as_object()? {
                        if !answers.contains_key(question) {
                            return None;
                        }
                        if let Some(notes) = annotation.get("notes") {
                            clean.insert(question.clone(), json!({"notes": notes.as_str()?}));
                        }
                    }
                    result["annotations"] = Value::Object(clean);
                }
                Some(result)
            }
            ("mcp", "accept")
                if self.details.get("mode").and_then(Value::as_str) == Some("url") =>
            {
                Some(json!({"outcome": outcome}))
            }
            ("mcp", "accept") => {
                let content = answer.get("content").cloned().unwrap_or(json!({}));
                if let Some(schema) = self.details.get("requestedSchema") {
                    if !valid_form(schema, &content) {
                        return None;
                    }
                } else if !content.is_object() {
                    return None;
                }
                Some(json!({"outcome": outcome, "content": content}))
            }
            _ => None,
        }
    }
}

fn valid_form(schema: &Value, content: &Value) -> bool {
    let Some(values) = content.as_object() else {
        return false;
    };
    let Some(properties) = schema.get("properties").and_then(Value::as_object) else {
        return values.is_empty();
    };
    if schema.get("type").and_then(Value::as_str) != Some("object") || properties.len() > 32 {
        return false;
    }
    if schema
        .get("required")
        .and_then(Value::as_array)
        .is_some_and(|required| {
            required
                .iter()
                .any(|name| name.as_str().is_none_or(|name| !values.contains_key(name)))
        })
    {
        return false;
    }
    values.iter().all(|(name, value)| {
        let Some(field) = properties.get(name) else {
            return false;
        };
        if field
            .get("enum")
            .and_then(Value::as_array)
            .is_some_and(|choices| !choices.contains(value))
        {
            return false;
        }
        match field.get("type").and_then(Value::as_str) {
            Some("string") => value.as_str().is_some_and(|text| text.len() <= 8192),
            Some("boolean") => value.is_boolean(),
            Some("number") => value.is_number(),
            Some("integer") => value.is_i64() || value.is_u64(),
            _ => false,
        }
    })
}

pub fn resolved(params: &Value) -> Option<(&str, &str)> {
    let session_id = params.get("sessionId")?.as_str()?;
    let update = params.get("update")?;
    if update.get("sessionUpdate")?.as_str()? != "interaction_resolved" {
        return None;
    }
    Some((
        session_id,
        update
            .get("tool_call_id")
            .or_else(|| update.get("toolCallId"))?
            .as_str()?,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn question_retains_rpc_id_and_rejects_unoffered_answers() {
        let request = PendingInteraction::parse("_x.ai/ask_user_question", json!(17), &json!({
            "method":"x.ai/ask_user_question", "params": {"sessionId":"s", "toolCallId":"t", "questions":[{"question":"Which?", "options":[{"label":"A"}]}]}
        })).unwrap();
        assert_eq!(request.request_id, json!(17));
        assert!(
            request
                .answer(&json!({"outcome":"accepted","answers":{"Which?":["B"]}}))
                .is_none()
        );
        assert!(
            request
                .answer(&json!({"outcome":"accepted","answers":{"Which?":["A"]}}))
                .is_some()
        );
        assert!(request.answer(&json!({"outcome":"approved"})).is_none());
    }
    #[test]
    fn plan_feedback_does_not_change_the_approval_outcome() {
        let request = PendingInteraction::parse(
            "x.ai/exit_plan_mode",
            json!("rpc"),
            &json!({"sessionId":"s","toolCallId":"t","planContent":"plan"}),
        )
        .unwrap();
        assert_eq!(
            request.answer(&json!({"outcome":"approved","feedback":"injected"})),
            Some(json!({"outcome":"approved"}))
        );
        assert!(request.answer(&json!({"outcome":"accepted"})).is_none());
    }
    #[test]
    fn mcp_form_rejects_missing_required_and_wrong_types() {
        let schema =
            json!({"type":"object","properties":{"count":{"type":"integer"}},"required":["count"]});
        assert!(!valid_form(&schema, &json!({})));
        assert!(!valid_form(&schema, &json!({"count":"two"})));
        assert!(!valid_form(&schema, &json!({"count":2,"extra":true})));
        assert!(valid_form(&schema, &json!({"count":2})));
    }
}
