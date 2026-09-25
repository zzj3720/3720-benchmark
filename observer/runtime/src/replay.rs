use std::io::Read;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use flate2::read::GzDecoder;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::canonical_json;

pub fn replay_projection(normalized: &[Value], start: usize, end: usize) -> Result<Value, String> {
    let start = start.min(normalized.len());
    let end = end.min(normalized.len()).max(start);
    let mut candidates = Vec::new();
    let mut skipped = 0_u64;
    let mut previous_state = normalized[..start].iter().rev().find_map(event_state);
    let mut previous_fingerprint = previous_state.as_ref().map(fingerprint).transpose()?;

    if let Some(state) = previous_state.clone() {
        let timestamp = normalized
            .get(start)
            .and_then(|event| event.get("timestamp_ms"))
            .cloned()
            .unwrap_or(Value::Null);
        let score = state
            .get("campaign")
            .and_then(|campaign| campaign.get("score"))
            .cloned()
            .unwrap_or(Value::Null);
        candidates.push(json!({
            "key": "initial",
            "event": {
                "sequence": 0,
                "timestamp_ms": timestamp,
                "type": "initial",
                "action": {"command": "initial"},
                "state": state,
                "result": {"ok": true},
                "score": score,
                "score_delta": 0
            },
            "operation_sequence": 0,
            "operation_timestamp_ms": timestamp,
            "operation_action": {"command": "initial"},
            "instruction_index": 0,
            "instruction_count": 0,
            "operation_size": 0,
            "has_instruction_trace": false
        }));
    }

    for operation in expanded_events(normalized, start, end) {
        let action = operation
            .get("action")
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default();
        let traced_steps = operation
            .get("steps")
            .and_then(Value::as_array)
            .map(Vec::as_slice)
            .unwrap_or_default();
        let steps = traced_steps
            .iter()
            .filter(|step| {
                step.get("state").is_some_and(Value::is_object)
                    || truthy_number(step.get("score_delta"))
            })
            .collect::<Vec<_>>();
        let argument_count = action
            .get("directions")
            .and_then(Value::as_array)
            .map(Vec::len)
            .or_else(|| {
                action
                    .get("argument_count")
                    .and_then(Value::as_u64)
                    .map(|n| n as usize)
            });
        let operation_size = if traced_steps.is_empty() {
            argument_count.filter(|count| *count > 0).unwrap_or(1)
        } else {
            traced_steps.len()
        };
        let visible = if steps.is_empty() {
            vec![&Value::Null]
        } else {
            steps
        };

        for (step_offset, step) in visible.iter().enumerate() {
            let step_object = step.as_object();
            let mut state = step_object
                .and_then(|value| value.get("state"))
                .filter(|value| value.is_object())
                .cloned()
                .or_else(|| {
                    operation
                        .get("state")
                        .filter(|value| value.is_object())
                        .cloned()
                });
            let score_delta = step_object
                .and_then(|value| value.get("score_delta"))
                .cloned()
                .or_else(|| operation.get("score_delta").cloned())
                .unwrap_or_else(|| Value::from(0));
            if state.is_none() && truthy_number(Some(&score_delta)) {
                state.clone_from(&previous_state);
            }
            let Some(state_value) = state else {
                skipped += 1;
                continue;
            };
            let state_fingerprint = fingerprint(&state_value)?;
            if previous_fingerprint.as_ref() == Some(&state_fingerprint)
                && !truthy_number(Some(&score_delta))
            {
                skipped += 1;
                continue;
            }
            let instruction_index = step_object
                .and_then(|value| value.get("index"))
                .cloned()
                .unwrap_or_else(|| Value::from(step_offset + 1));
            let sequence = operation.get("sequence").cloned().unwrap_or(Value::Null);
            let timestamp = operation
                .get("timestamp_ms")
                .cloned()
                .unwrap_or(Value::Null);
            let event_action = step_object
                .and_then(|value| value.get("action"))
                .cloned()
                .unwrap_or_else(|| Value::Object(action.clone()));
            let result = step_object
                .and_then(|value| value.get("result"))
                .cloned()
                .or_else(|| operation.get("result").cloned())
                .unwrap_or(Value::Null);
            let score = step_object
                .and_then(|value| value.get("score"))
                .cloned()
                .or_else(|| operation.get("score").cloned())
                .unwrap_or(Value::Null);
            let key = if traced_steps.is_empty() {
                display_json(&sequence)
            } else {
                format!(
                    "{}:{}",
                    display_json(&sequence),
                    display_json(&instruction_index)
                )
            };
            candidates.push(json!({
                "key": key,
                "event": {
                    "sequence": sequence,
                    "timestamp_ms": timestamp,
                    "type": operation.get("type").cloned().unwrap_or(Value::Null),
                    "action": event_action,
                    "state": state_value,
                    "result": result,
                    "score": score,
                    "score_delta": score_delta
                },
                "operation_sequence": sequence,
                "operation_timestamp_ms": timestamp,
                "operation_action": action,
                "instruction_index": instruction_index,
                "instruction_count": if traced_steps.is_empty() { 1 } else { traced_steps.len() },
                "operation_size": operation_size,
                "has_instruction_trace": !traced_steps.is_empty()
            }));
            previous_state = Some(state_value);
            previous_fingerprint = Some(state_fingerprint);
        }
    }

    // Undo/redo manipulates indices, not copies of every retained game state.
    // Every frame is kept; `undone` marks the ones the default view leaves out:
    // a branch the agent undid, the undo/redo that moved over it, and resets.
    // An undo whose target is not on this page stays visible, so the board
    // steps back instead of jumping.
    let fingerprints = candidates
        .iter()
        .map(|frame| frame.pointer("/event/state").map(fingerprint).transpose())
        .collect::<Result<Vec<_>, _>>()?;
    let mut undone = vec![false; candidates.len()];
    let mut output = Vec::<usize>::new();
    let mut redo = Vec::<Vec<usize>>::new();
    let mut index = 0;
    while index < candidates.len() {
        let sequence = candidates[index].get("operation_sequence");
        let mut boundary = index + 1;
        while boundary < candidates.len()
            && candidates[boundary].get("operation_sequence") == sequence
        {
            boundary += 1;
        }
        let command = candidates[index]
            .pointer("/operation_action/command")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_ascii_lowercase();
        let target = fingerprints[boundary - 1].as_ref();
        match command.as_str() {
            "undo" => {
                if let Some(matching) = target.and_then(|target| {
                    output
                        .iter()
                        .rposition(|index| fingerprints[*index].as_ref() == Some(target))
                }) {
                    let removed = output.split_off(matching + 1);
                    for removed in &removed {
                        undone[*removed] = true;
                    }
                    undone[index..boundary].fill(true);
                    if !removed.is_empty() {
                        redo.push(removed);
                    }
                } else {
                    output.extend(index..boundary);
                }
            }
            "redo" => {
                if let Some(restored) = redo.pop() {
                    let matching = target
                        .and_then(|target| {
                            restored
                                .iter()
                                .position(|index| fingerprints[*index].as_ref() == Some(target))
                        })
                        .unwrap_or(restored.len().saturating_sub(1));
                    for restored in restored.iter().take(matching + 1) {
                        undone[*restored] = false;
                    }
                    output.extend(restored.into_iter().take(matching + 1));
                    undone[index..boundary].fill(true);
                } else {
                    output.extend(index..boundary);
                }
            }
            "restart" | "reset" => {
                redo.clear();
                undone[index..boundary].fill(true);
            }
            _ => {
                redo.clear();
                output.extend(index..boundary);
            }
        }
        index = boundary;
    }
    let mut frames = candidates;
    for (frame, undone) in frames.iter_mut().zip(&undone) {
        frame["undone"] = Value::Bool(*undone);
    }
    let eliminated = undone.iter().filter(|undone| **undone).count();
    let mut operations: Vec<Value> = Vec::new();
    for frame in &frames {
        if frame["undone"] == Value::Bool(true)
            || frame
                .pointer("/operation_action/command")
                .and_then(Value::as_str)
                == Some("initial")
        {
            continue;
        }
        let sequence = frame
            .get("operation_sequence")
            .cloned()
            .unwrap_or(Value::Null);
        if operations
            .last()
            .and_then(|operation| operation.get("sequence"))
            == Some(&sequence)
        {
            let operation = operations.last_mut().expect("last operation");
            operation["last_frame_key"] = frame["key"].clone();
            operation["frame_count"] =
                Value::from(operation["frame_count"].as_u64().unwrap_or(0) + 1);
            operation["score_delta"] = Value::from(
                operation["score_delta"].as_i64().unwrap_or(0)
                    + frame
                        .pointer("/event/score_delta")
                        .and_then(Value::as_i64)
                        .unwrap_or(0),
            );
            continue;
        }
        operations.push(json!({
            "sequence": sequence,
            "timestamp_ms": frame.get("operation_timestamp_ms").cloned().unwrap_or(Value::Null),
            "action": frame.get("operation_action").cloned().unwrap_or_else(|| json!({})),
            "first_frame_key": frame.get("key").cloned().unwrap_or(Value::Null),
            "last_frame_key": frame.get("key").cloned().unwrap_or(Value::Null),
            "frame_count": 1,
            "score_delta": frame.pointer("/event/score_delta").and_then(Value::as_i64).unwrap_or(0)
        }));
    }

    let mut result = json!({"skipped_unchanged":skipped,"eliminated_history_frames":eliminated});
    result["frames"] = Value::Array(frames);
    result["operations"] = Value::Array(operations);
    Ok(result)
}

fn expanded_events(
    normalized: &[Value],
    start: usize,
    end: usize,
) -> impl Iterator<Item = Value> + '_ {
    let mut current_state = normalized[..start].iter().rev().find_map(event_state);
    normalized[start..end].iter().map(move |event| {
        let mut expanded = event.as_object().cloned().unwrap_or_default();
        let has_steps = expanded
            .get("steps")
            .and_then(Value::as_array)
            .is_some_and(|steps| !steps.is_empty());
        if !has_steps {
            expanded.insert(
                "steps".into(),
                Value::Array(decode_instruction_trace(
                    expanded.get("instruction_trace"),
                    expanded.get("score"),
                )),
            );
        }
        if let Some(last_state) = expanded
            .get("steps")
            .and_then(Value::as_array)
            .filter(|steps| !steps.is_empty())
            .and_then(|steps| steps.last())
            .and_then(|step| step.get("state"))
            .cloned()
        {
            current_state = Some(last_state.clone());
            expanded.insert("state".into(), last_state);
        } else if let Some(snapshot) = decode_state_snapshot(expanded.get("state_snapshot")) {
            current_state = Some(snapshot.clone());
            expanded.insert("state".into(), snapshot);
        } else if expanded.get("state").is_some_and(nonempty_object) {
            current_state = expanded.get("state").cloned();
        } else if let Some(state) = current_state.clone() {
            expanded.insert("state".into(), state);
        }
        expanded.remove("instruction_trace");
        expanded.remove("state_snapshot");
        Value::Object(expanded)
    })
}

fn event_state(event: &Value) -> Option<Value> {
    let steps = event
        .get("steps")
        .and_then(Value::as_array)
        .filter(|steps| !steps.is_empty())
        .cloned()
        .unwrap_or_else(|| {
            decode_instruction_trace(event.get("instruction_trace"), event.get("score"))
        });
    if let Some(state) = steps
        .last()
        .and_then(|step| step.get("state"))
        .filter(|state| nonempty_object(state))
    {
        return Some(state.clone());
    }
    decode_state_snapshot(event.get("state_snapshot")).or_else(|| {
        event
            .get("state")
            .filter(|state| nonempty_object(state))
            .cloned()
    })
}

fn decode_instruction_trace(trace: Option<&Value>, default_score: Option<&Value>) -> Vec<Value> {
    let Some(decoded) = decode_gzip_json(trace) else {
        return Vec::new();
    };
    let Some(candidates) = decoded.as_array() else {
        return Vec::new();
    };
    candidates
        .iter()
        .enumerate()
        .filter_map(|(offset, candidate)| {
            let object = candidate.as_object()?;
            let state = object.get("state").filter(|state| state.is_object())?;
            Some(json!({
                "index": object.get("index").cloned().unwrap_or_else(|| Value::from(offset + 1)),
                "action": object.get("action").cloned().unwrap_or(Value::Null),
                "state": state,
                "result": object.get("result").cloned().unwrap_or_else(|| json!({"ok": true})),
                "score": object.get("score").cloned().or_else(|| default_score.cloned()).unwrap_or_else(|| Value::from(0)),
                "score_delta": object.get("score_delta").cloned().unwrap_or_else(|| Value::from(0))
            }))
        })
        .collect()
}

fn decode_state_snapshot(snapshot: Option<&Value>) -> Option<Value> {
    decode_gzip_json(snapshot).filter(nonempty_object)
}

fn decode_gzip_json(encoded: Option<&Value>) -> Option<Value> {
    let encoded = encoded?.as_object()?;
    if encoded.get("encoding")?.as_str()? != "gzip+base64" {
        return None;
    }
    let compressed = BASE64.decode(encoded.get("data")?.as_str()?).ok()?;
    let mut decoded = Vec::new();
    GzDecoder::new(compressed.as_slice())
        .take(64 * 1024 * 1024 + 1)
        .read_to_end(&mut decoded)
        .ok()?;
    if decoded.len() > 64 * 1024 * 1024 {
        return None;
    }
    if encoded
        .get("uncompressed_bytes")
        .and_then(Value::as_u64)
        .is_some_and(|expected| expected != decoded.len() as u64)
    {
        return None;
    }
    serde_json::from_slice(&decoded).ok()
}

fn fingerprint(value: &Value) -> Result<Vec<u8>, String> {
    canonical_json(value).map(|bytes| Sha256::digest(bytes).to_vec())
}

fn nonempty_object(value: &Value) -> bool {
    value.as_object().is_some_and(|object| !object.is_empty())
}

fn truthy_number(value: Option<&Value>) -> bool {
    value
        .and_then(Value::as_f64)
        .is_some_and(|number| number != 0.0)
}

fn display_json(value: &Value) -> String {
    match value {
        Value::String(value) => value.clone(),
        Value::Null => "None".into(),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state(value: &str) -> Value {
        json!({"space": {"map": [[value]]}})
    }

    #[test]
    fn projection_eliminates_undo_redo_and_meta_frames() {
        let events = vec![
            json!({"sequence": 1, "action": {"command": "reset"}, "state": state("A")}),
            json!({"sequence": 2, "action": {"command": "move"}, "state": state("B")}),
            json!({"sequence": 3, "action": {"command": "move"}, "state": state("C")}),
            json!({"sequence": 4, "action": {"command": "undo"}, "state": state("B")}),
            json!({"sequence": 5, "action": {"command": "move"}, "state": state("D")}),
            json!({"sequence": 6, "action": {"command": "undo"}, "state": state("B")}),
            json!({"sequence": 7, "action": {"command": "redo"}, "state": state("D")}),
            json!({"sequence": 8, "action": {"command": "move"}, "state": state("E"), "score_delta": 1}),
        ];
        let projection = replay_projection(&events, 1, 8).unwrap();
        let keys = |undone: Option<bool>| {
            projection["frames"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|frame| undone.is_none_or(|undone| frame["undone"] == undone))
                .map(|frame| frame["key"].as_str().unwrap())
                .collect::<Vec<_>>()
        };
        assert_eq!(keys(Some(false)), ["initial", "2", "5", "8"]);
        assert_eq!(keys(None), ["initial", "2", "3", "4", "5", "6", "7", "8"]);
        assert_eq!(projection["eliminated_history_frames"], 4);
        assert_eq!(projection["operations"].as_array().unwrap().len(), 3);
    }

    #[test]
    fn undo_past_the_page_start_stays_visible() {
        // The page starts at B; the undo returns to A, which is not on it.
        let events = vec![
            json!({"sequence": 1, "action": {"command": "move"}, "state": state("B")}),
            json!({"sequence": 2, "action": {"command": "undo"}, "state": state("A")}),
            json!({"sequence": 3, "action": {"command": "move"}, "state": state("C")}),
        ];
        let projection = replay_projection(&events, 1, 3).unwrap();
        let visible = projection["frames"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|frame| frame["undone"] == false)
            .map(|frame| frame["key"].as_str().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(visible, ["initial", "2", "3"]);
    }
}
