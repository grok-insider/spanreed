use serde_json::Value;

use crate::acp::AcpError;
use crate::dispatch::{DispatchError, DispatchOutcome};
use crate::models::SessionModelState;
use crate::protocol::{CommandEnvelope, Event, Operation};
use crate::server::HostState;

pub(crate) async fn capture_open(state: &HostState, session_id: &str, value: &Value) {
    let changed = {
        let mut models = state.model_states.lock().await;
        let old = models.entry(session_id.into()).or_default();
        let mut next = SessionModelState::default();
        next.capture(value);
        next.revision = old.revision;
        if *old != next {
            next.revision = old.revision.saturating_add(1);
            *old = next;
            true
        } else {
            false
        }
    };
    if changed {
        state
            .emit_event(
                Event::SessionModelsChanged {
                    session_id: session_id.into(),
                },
                None,
            )
            .await;
    }
}

pub(crate) async fn capture_update(state: &HostState, params: &Value) {
    let Some(session_id) = params.get("sessionId").and_then(Value::as_str) else {
        return;
    };
    let changed = {
        let mut models = state.model_states.lock().await;
        models
            .get_mut(session_id)
            .is_some_and(|model| model.capture_update(params))
    };
    if changed {
        if state
            .model_states
            .lock()
            .await
            .get(session_id)
            .is_some_and(|model| !model.uncertain)
        {
            if state
                .execution
                .lock()
                .await
                .reconcile_configuration(session_id)
                .is_ok()
            {
                state
                    .emit_event(
                        Event::SessionExecutionChanged {
                            session_id: session_id.into(),
                        },
                        None,
                    )
                    .await;
            }
        }
        state
            .emit_event(
                Event::SessionModelsChanged {
                    session_id: session_id.into(),
                },
                None,
            )
            .await;
    }
}

pub(crate) async fn command(
    state: &HostState,
    envelope: &CommandEnvelope,
) -> Result<DispatchOutcome, DispatchError> {
    match &envelope.operation {
        Operation::GetSessionModels { session_id } => {
            state.session.lock().await.session(session_id)?;
            let model = state
                .model_states
                .lock()
                .await
                .get(session_id)
                .cloned()
                .unwrap_or_default();
            Ok(DispatchOutcome::SessionModels {
                session_id: session_id.clone(),
                state: model,
            })
        }
        Operation::SetSessionConfiguration {
            session_id,
            expected_config_revision,
            model_id,
            reasoning_effort,
        } => {
            state.session.lock().await.session(session_id)?;
            state
                .execution
                .lock()
                .await
                .begin_configuration(session_id)
                .map_err(DispatchError::Refused)?;
            let before = state
                .model_states
                .lock()
                .await
                .get(session_id)
                .cloned()
                .unwrap_or_default();
            let validation = validate_change(
                &before,
                *expected_config_revision,
                model_id.as_deref(),
                reasoning_effort.as_deref(),
            );
            if let Err(error) = validation {
                state
                    .execution
                    .lock()
                    .await
                    .finish_configuration(session_id, false)
                    .map_err(DispatchError::Refused)?;
                return Err(error);
            }
            let Some(agent) = state.agent.lock().await.clone() else {
                state
                    .execution
                    .lock()
                    .await
                    .finish_configuration(session_id, false)
                    .map_err(DispatchError::Refused)?;
                return Err(DispatchError::NoSession);
            };
            let result = if let Some(model) = model_id {
                if before.use_model_config {
                    agent.set_model_config(session_id, model).await
                } else {
                    agent.set_session_model(session_id, model, None).await
                }
            } else {
                agent
                    .set_reasoning_effort(
                        session_id,
                        reasoning_effort.as_deref().unwrap_or_default(),
                    )
                    .await
            };
            let mut next = state.model_states.lock().await;
            let model = next.entry(session_id.clone()).or_default();
            let outcome = match result {
                Ok(value) => {
                    model.capture(&value);
                    let confirmed = model_id.as_ref().map_or_else(
                        || reasoning_effort.as_ref() == model.current_effort.as_ref(),
                        |id| model.current_model.as_ref() == Some(id),
                    );
                    if confirmed {
                        model.uncertain = false;
                        Ok(())
                    } else {
                        model.uncertain = true;
                        model.revision = model.revision.saturating_add(1);
                        Err(DispatchError::Agent)
                    }
                }
                Err(AcpError::InvalidModel) => Err(DispatchError::Refused("model_unavailable")),
                Err(error) if error.is_unsupported_method() => Err(DispatchError::Unsupported),
                Err(_) => {
                    model.uncertain = true;
                    model.revision = model.revision.saturating_add(1);
                    Err(DispatchError::Agent)
                }
            };
            let projected = model.clone();
            drop(next);
            state
                .execution
                .lock()
                .await
                .finish_configuration(session_id, projected.uncertain)
                .map_err(DispatchError::Refused)?;
            state
                .emit_event(
                    Event::SessionModelsChanged {
                        session_id: session_id.clone(),
                    },
                    None,
                )
                .await;
            state
                .emit_event(
                    Event::SessionExecutionChanged {
                        session_id: session_id.clone(),
                    },
                    None,
                )
                .await;
            outcome?;
            if let Some(model) = &projected.current_model {
                state
                    .session
                    .lock()
                    .await
                    .set_review_model(session_id, model);
            }
            Ok(DispatchOutcome::SessionModels {
                session_id: session_id.clone(),
                state: projected,
            })
        }
        _ => Err(DispatchError::Unsupported),
    }
}

fn validate_change(
    state: &SessionModelState,
    revision: u64,
    model: Option<&str>,
    effort: Option<&str>,
) -> Result<(), DispatchError> {
    if state.revision != revision {
        return Err(DispatchError::Refused("configuration_conflict"));
    }
    if state.uncertain {
        return Err(DispatchError::Refused("configuration_uncertain"));
    }
    match (model, effort) {
        (Some(id), None)
            if state.source == "live"
                && state.can_set_model
                && state.selectable_models.iter().any(|model| model.id == id) =>
        {
            Ok(())
        }
        (Some(_), None) => Err(DispatchError::Refused("model_unavailable")),
        (None, Some(id))
            if state.can_set_effort
                && state.effort_options.iter().any(|effort| effort.id == id) =>
        {
            Ok(())
        }
        (None, Some(_)) => Err(DispatchError::Refused("effort_not_supported")),
        _ => Err(DispatchError::Refused("invalid_configuration_change")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn live_model_and_effort_are_separate_revision_checked_changes() {
        let mut state = SessionModelState::default();
        state.capture(&json!({"models":{"currentModelId":"relay/custom","availableModels":[{"modelId":"relay/custom","name":"Custom"}]},"configOptions":[{"id":"reasoning_effort","currentValue":"balanced","options":[{"value":"balanced","name":"Balanced"}]}]}));
        assert!(validate_change(&state, state.revision, Some("relay/custom"), None).is_ok());
        assert!(validate_change(&state, state.revision, None, Some("balanced")).is_ok());
        assert_eq!(
            validate_change(
                &state,
                state.revision,
                Some("relay/custom"),
                Some("balanced")
            ),
            Err(DispatchError::Refused("invalid_configuration_change"))
        );
        assert_eq!(
            validate_change(&state, state.revision - 1, Some("relay/custom"), None),
            Err(DispatchError::Refused("configuration_conflict"))
        );
        assert_eq!(
            validate_change(&state, state.revision, Some("grok-invented"), None),
            Err(DispatchError::Refused("model_unavailable"))
        );
        assert_eq!(
            validate_change(&state, state.revision, None, Some("high")),
            Err(DispatchError::Refused("effort_not_supported"))
        );
    }

    #[test]
    fn missing_catalogue_and_uncertain_state_never_authorize_changes() {
        let mut state = SessionModelState::default();
        assert!(validate_change(&state, 0, Some("grok-4.7"), None).is_err());
        state.uncertain = true;
        assert_eq!(
            validate_change(&state, 0, Some("grok-4.7"), None),
            Err(DispatchError::Refused("configuration_uncertain"))
        );
    }
}
