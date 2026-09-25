//! Versioned observer context. New journal payloads carry this small projection
//! alongside object references. Legacy events enter through the same adapter.
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

pub const CONTEXT_SCHEMA: &str = "benchmark-observation-context-v1";

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Context {
    pub schema: String,
    pub kind: String,
    pub reference: String,
    pub title: String,
    pub boundary: String,
    pub complete: bool,
}

pub fn context(task: &str, event: &Value, state: &Value) -> Context {
    if let Some(value) = event.get("context")
        && let Ok(context) = serde_json::from_value::<Context>(value.clone())
        && context.schema == CONTEXT_SCHEMA
        && !context.reference.is_empty()
    {
        return context;
    }
    // Earlier Sausage events already carry the action's *pre-state* context.
    // Preserve that attribution when a winning move exits to the overworld.
    if let Some(previous) = event.get("context")
        && let Some(reference) = text(previous.get("reference"))
        && let Some(kind) = text(previous.get("kind"))
    {
        return Context {
            schema: CONTEXT_SCHEMA.into(),
            kind: if kind == "complete" { "world" } else { kind }.into(),
            reference: reference.into(),
            title: text(previous.get("title")).unwrap_or(reference).into(),
            boundary: if kind == "level" { "score" } else { "episode" }.into(),
            complete: kind == "complete",
        };
    }
    let mut result = Context {
        schema: CONTEXT_SCHEMA.into(),
        kind: "level".into(),
        reference: "session".into(),
        title: "当前任务".into(),
        boundary: "score".into(),
        complete: false,
    };
    if state.get("mode").and_then(Value::as_str) == Some("overworld") {
        result.kind = "overworld".into();
        result.reference = format!(
            "overworld:{}",
            state
                .pointer("/campaign/score")
                .and_then(Value::as_i64)
                .unwrap_or(0)
        );
        result.title = text(state.pointer("/overworld/title"))
            .unwrap_or("Land's End")
            .into();
        result.boundary = "episode".into();
    } else if state.get("shift").is_some()
        || matches!(
            task,
            "kitchen-terminal" | "emergency-operator" | "overcooked"
        )
    {
        result.kind = "shift".into();
        let shift = text(state.pointer("/shift/id"))
            .map(str::to_owned)
            .unwrap_or_else(|| {
                format!(
                    "{}:{}",
                    state
                        .pointer("/campaign/level")
                        .and_then(Value::as_u64)
                        .unwrap_or(1),
                    text(state.pointer("/campaign/scene")).unwrap_or("shift")
                )
            });
        result.reference = format!("shift:{shift}");
        result.title = text(state.pointer("/shift/title"))
            .or_else(|| text(state.pointer("/campaign/scene")))
            .unwrap_or("班次")
            .into();
        result.boundary = "episode".into();
        result.complete = text(state.pointer("/shift/status")) == Some("complete");
    } else if task == "swarm-farming" || state.get("robots").is_some() {
        result.kind = "world".into();
        result.reference = "world:swarm".into();
        result.title = "Swarm Farming".into();
        result.boundary = "episode".into();
        result.complete = state.get("won").and_then(Value::as_bool).unwrap_or(false);
    } else {
        result.reference = text(state.pointer("/level/reference"))
            .or_else(|| text(state.pointer("/level/id")))
            .or_else(|| text(event.get("selected")))
            .unwrap_or("session")
            .into();
        result.title = text(state.pointer("/level/title"))
            .unwrap_or(&result.reference)
            .into();
        result.complete = state
            .pointer("/level/solved")
            .and_then(Value::as_bool)
            .unwrap_or(false);
    }
    result
}

pub fn stamp(event: &mut Value) {
    if !event.is_object() {
        return;
    }
    if !event.get("state").is_some_and(Value::is_object)
        && event.get("selected").is_none()
        && event.get("context").is_none()
    {
        return;
    }
    let task = text(event.pointer("/task/id")).unwrap_or_default();
    let state = event.get("state").unwrap_or(&Value::Null);
    let context = context(task, event, state);
    event["context"] = json!(context);
}

fn text(value: Option<&Value>) -> Option<&str> {
    value.and_then(Value::as_str).filter(|s| !s.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn legacy_pre_action_context_survives_a_winning_transition() {
        let context = context(
            "sausage-roll",
            &json!({"context":{"kind":"level","reference":"puzzle-one","title":"Puzzle One"}}),
            &json!({"mode":"overworld","campaign":{"score":1}}),
        );
        assert_eq!(context.kind, "level");
        assert_eq!(context.reference, "puzzle-one");
        assert_eq!(context.boundary, "score");
    }
    #[test]
    fn contexts_identify_levels_shifts_and_worlds() {
        for (task, state, kind, reference) in [
            ("sokoban", json!({"level":{"id":"one"}}), "level", "one"),
            (
                "parabox-intro",
                json!({"level":{"reference":"a1"}}),
                "level",
                "a1",
            ),
            (
                "minesweeper",
                json!({"level":{"id":"field"}}),
                "level",
                "field",
            ),
            (
                "emergency-operator",
                json!({"shift":{"id":"career"}}),
                "shift",
                "shift:career",
            ),
            (
                "kitchen-terminal",
                json!({"campaign":{"level":2,"scene":"soup"},"shift":{}}),
                "shift",
                "shift:2:soup",
            ),
            (
                "swarm-farming",
                json!({"robots":[]}),
                "world",
                "world:swarm",
            ),
            (
                "sausage-roll",
                json!({"mode":"overworld","campaign":{"score":3}}),
                "overworld",
                "overworld:3",
            ),
        ] {
            let c = context(task, &Value::Null, &state);
            assert_eq!((c.kind.as_str(), c.reference.as_str()), (kind, reference));
        }
    }
}
