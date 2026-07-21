use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use tiny_http::{Header, Method, Request, Response, Server, StatusCode};

const EVENT_SCHEMA: &str = "benchmark-observer-event-v1";
const BATCH_SCHEMA: &str = "benchmark-observer-batch-v1";

#[derive(Clone)]
struct Config {
    source: PathBuf,
    format: SourceFormat,
    task_id: String,
    task_label: String,
    listen: String,
}

#[derive(Clone, Copy)]
enum SourceFormat {
    Generic,
    Parabox,
    Swarm,
}

fn main() {
    if let Err(error) = serve() {
        eprintln!("observer-relay: {error}");
        std::process::exit(2);
    }
}

fn serve() -> Result<(), String> {
    let config = Config::from_env()?;
    let server = Server::http(&config.listen)
        .map_err(|error| format!("could not listen on {}: {error}", config.listen))?;
    println!(
        "observer-relay listening on {} for {}",
        config.listen, config.task_id
    );
    for request in server.incoming_requests() {
        let config = config.clone();
        thread::spawn(move || {
            if let Err(error) = handle(request, &config) {
                eprintln!("observer-relay: {error}");
            }
        });
    }
    Ok(())
}

impl Config {
    fn from_env() -> Result<Self, String> {
        let source = env::var_os("OBSERVER_SOURCE")
            .map(PathBuf::from)
            .ok_or_else(|| "OBSERVER_SOURCE is required".to_owned())?;
        let format = match env::var("OBSERVER_FORMAT")
            .unwrap_or_else(|_| "generic".to_owned())
            .as_str()
        {
            "generic" => SourceFormat::Generic,
            "parabox" => SourceFormat::Parabox,
            "swarm" => SourceFormat::Swarm,
            value => return Err(format!("unknown OBSERVER_FORMAT {value:?}")),
        };
        let task_id =
            env::var("OBSERVER_TASK_ID").map_err(|_| "OBSERVER_TASK_ID is required".to_owned())?;
        let task_label = env::var("OBSERVER_TASK_LABEL").unwrap_or_else(|_| task_id.clone());
        let listen = env::var("OBSERVER_LISTEN_ADDR").unwrap_or_else(|_| "0.0.0.0:3721".to_owned());
        Ok(Self {
            source,
            format,
            task_id,
            task_label,
            listen,
        })
    }

    fn task(&self) -> Value {
        json!({
            "id": self.task_id,
            "label": self.task_label,
            "kind": "game",
        })
    }
}

fn handle(request: Request, config: &Config) -> Result<(), String> {
    let (path, query) = split_url(request.url());
    if request.method() == &Method::Options {
        return respond(request, StatusCode(204), Value::Null);
    }
    if request.method() == &Method::Get && path == "/health" {
        return respond(
            request,
            StatusCode(200),
            json!({"ok": true, "task": config.task()}),
        );
    }
    if request.method() == &Method::Get && path == "/v1/observe/snapshot" {
        return match normalized_events(config) {
            Ok(events) => {
                let latest = latest_sequence(&events);
                let state = events
                    .last()
                    .and_then(|event| event.get("state"))
                    .cloned()
                    .unwrap_or(Value::Null);
                respond(
                    request,
                    StatusCode(200),
                    json!({
                        "schema": "benchmark-observer-snapshot-v1",
                        "task": config.task(),
                        "latest_sequence": latest,
                        "state": state,
                    }),
                )
            }
            Err(error) => respond_error(request, StatusCode(503), error),
        };
    }
    if request.method() == &Method::Get && path == "/v1/observe/events" {
        let parameters = query_parameters(query);
        let after = parameter_u64(&parameters, "after", 0)?;
        let limit = parameter_u64(&parameters, "limit", 128)?.clamp(1, 1000) as usize;
        let wait_ms = parameter_u64(&parameters, "wait_ms", 0)?.min(30_000);
        let deadline = Instant::now() + Duration::from_millis(wait_ms);
        loop {
            match normalized_events(config) {
                Ok(events) => {
                    let latest = latest_sequence(&events);
                    if latest > after || Instant::now() >= deadline {
                        let selected = events
                            .into_iter()
                            .filter(|event| {
                                event
                                    .get("sequence")
                                    .and_then(Value::as_u64)
                                    .is_some_and(|sequence| sequence > after)
                            })
                            .take(limit)
                            .collect::<Vec<_>>();
                        return respond(
                            request,
                            StatusCode(200),
                            json!({
                                "schema": BATCH_SCHEMA,
                                "task": config.task(),
                                "after": after,
                                "latest_sequence": latest,
                                "events": selected,
                            }),
                        );
                    }
                }
                Err(error) if wait_ms == 0 || Instant::now() >= deadline => {
                    return respond_error(request, StatusCode(503), error);
                }
                Err(_) => {}
            }
            thread::sleep(Duration::from_millis(100));
        }
    }
    respond_error(request, StatusCode(404), "unknown endpoint".to_owned())
}

fn normalized_events(config: &Config) -> Result<Vec<Value>, String> {
    let values = read_json_lines(&config.source)?;
    match config.format {
        SourceFormat::Generic => normalize_generic(values),
        SourceFormat::Parabox => normalize_parabox(config, values),
        SourceFormat::Swarm => normalize_swarm(config, values),
    }
}

fn normalize_generic(values: Vec<Value>) -> Result<Vec<Value>, String> {
    for value in &values {
        if value.get("schema").and_then(Value::as_str) != Some(EVENT_SCHEMA)
            || value.get("sequence").and_then(Value::as_u64).is_none()
            || value.get("state").is_none()
        {
            return Err("generic event source contains an invalid event".to_owned());
        }
    }
    Ok(values)
}

fn normalize_parabox(config: &Config, values: Vec<Value>) -> Result<Vec<Value>, String> {
    values
        .into_iter()
        .enumerate()
        .map(|(index, source)| {
            if source.get("schema").and_then(Value::as_str) != Some("parabox-events-v1") {
                return Err(format!("invalid Parabox event at line {}", index + 1));
            }
            let event_type = source
                .get("type")
                .and_then(Value::as_str)
                .unwrap_or("event");
            Ok(json!({
                "schema": EVENT_SCHEMA,
                "sequence": index + 1,
                "timestamp_ms": source.get("timestamp_ms").cloned().unwrap_or(Value::Null),
                "task": config.task(),
                "type": event_type,
                "action": if event_type == "request" {
                    json!({
                        "command": source.get("command").cloned().unwrap_or(Value::Null),
                        "argument_count": source.get("argument_count").cloned().unwrap_or(Value::Null),
                    })
                } else {
                    Value::Null
                },
                "state": source.get("state").cloned().unwrap_or(Value::Null),
                "result": {
                    "ok": source.get("ok").cloned().unwrap_or(Value::Bool(true)),
                    "score_delta": source.get("score_delta").cloned().unwrap_or(Value::from(0)),
                    "solved_levels": source.get("solved_levels").cloned().unwrap_or_else(|| json!([])),
                },
            }))
        })
        .collect()
}

fn normalize_swarm(config: &Config, values: Vec<Value>) -> Result<Vec<Value>, String> {
    let (header, records) = values
        .split_first()
        .ok_or_else(|| "Swarm audit is empty".to_owned())?;
    if header.get("schema").and_then(Value::as_str) != Some("swarm-audit-v1") {
        return Err("invalid Swarm audit header".to_owned());
    }
    let initial = header
        .get("initial_state")
        .cloned()
        .ok_or_else(|| "Swarm audit header has no initial_state".to_owned())?;
    let mut events = vec![json!({
        "schema": EVENT_SCHEMA,
        "sequence": 1,
        "timestamp_ms": Value::Null,
        "task": config.task(),
        "type": "sidecar_started",
        "action": Value::Null,
        "state": initial,
        "result": {"ok": true},
    })];
    for (index, source) in records.iter().enumerate() {
        let sequence = source
            .get("sequence")
            .and_then(Value::as_u64)
            .ok_or_else(|| format!("invalid Swarm audit record at line {}", index + 2))?;
        let response = source
            .get("response")
            .ok_or_else(|| format!("Swarm audit record {} has no response", sequence))?;
        events.push(json!({
            "schema": EVENT_SCHEMA,
            "sequence": sequence + 1,
            "timestamp_ms": source.get("timestamp_ms").cloned().unwrap_or(Value::Null),
            "task": config.task(),
            "type": "command",
            "action": {
                "command": source.get("command").cloned().unwrap_or(Value::Null),
                "argument": source.get("argument").cloned().unwrap_or(Value::Null),
            },
            "state": response.get("data").cloned().unwrap_or(Value::Null),
            "result": {
                "ok": response.get("ok").cloned().unwrap_or(Value::Bool(false)),
                "command": response.get("command").cloned().unwrap_or(Value::Null),
            },
        }));
    }
    Ok(events)
}

fn read_json_lines(path: &Path) -> Result<Vec<Value>, String> {
    let source = fs::read_to_string(path)
        .map_err(|error| format!("could not read {}: {error}", path.display()))?;
    let complete = source.ends_with('\n');
    let lines = source.split_terminator('\n').collect::<Vec<_>>();
    let mut values = Vec::with_capacity(lines.len());
    for (index, line) in lines.iter().enumerate() {
        if line.is_empty() {
            continue;
        }
        match serde_json::from_str(line) {
            Ok(value) => values.push(value),
            Err(_) if index + 1 == lines.len() && !complete => break,
            Err(error) => {
                return Err(format!(
                    "invalid {} line {}: {error}",
                    path.display(),
                    index + 1
                ));
            }
        }
    }
    Ok(values)
}

fn latest_sequence(events: &[Value]) -> u64 {
    events
        .last()
        .and_then(|event| event.get("sequence"))
        .and_then(Value::as_u64)
        .unwrap_or(0)
}

fn split_url(url: &str) -> (&str, &str) {
    url.split_once('?').map_or((url, ""), |parts| parts)
}

fn query_parameters(query: &str) -> Vec<(&str, &str)> {
    query
        .split('&')
        .filter_map(|item| item.split_once('='))
        .collect()
}

fn parameter_u64(parameters: &[(&str, &str)], name: &str, default: u64) -> Result<u64, String> {
    let Some((_, value)) = parameters.iter().find(|(key, _)| *key == name) else {
        return Ok(default);
    };
    value
        .parse()
        .map_err(|_| format!("query parameter {name} must be a non-negative integer"))
}

fn respond_error(request: Request, status: StatusCode, message: String) -> Result<(), String> {
    respond(
        request,
        status,
        json!({"ok": false, "error": {"message": message}}),
    )
}

fn respond(request: Request, status: StatusCode, body: Value) -> Result<(), String> {
    let bytes = serde_json::to_vec(&body)
        .map_err(|error| format!("could not serialize response: {error}"))?;
    request
        .respond(
            Response::from_data(bytes)
                .with_status_code(status)
                .with_header(
                    Header::from_bytes("Content-Type", "application/json; charset=utf-8")
                        .expect("static header"),
                )
                .with_header(
                    Header::from_bytes("Access-Control-Allow-Origin", "*").expect("static header"),
                ),
        )
        .map_err(|error| format!("could not send response: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_swarm_audit_to_the_common_protocol() {
        let config = Config {
            source: PathBuf::new(),
            format: SourceFormat::Swarm,
            task_id: "swarm-farming".to_owned(),
            task_label: "Swarm Farming".to_owned(),
            listen: String::new(),
        };
        let values = vec![
            json!({"schema": "swarm-audit-v1", "initial_state": {"tick": 0}}),
            json!({
                "sequence": 1,
                "command": "advance",
                "argument": {"ticks": 10},
                "response": {"ok": true, "command": "advance", "data": {"tick": 10}},
            }),
        ];
        let events = normalize_swarm(&config, values).expect("events");
        assert_eq!(events.len(), 2);
        assert_eq!(events[1]["schema"], EVENT_SCHEMA);
        assert_eq!(events[1]["sequence"], 2);
        assert_eq!(events[1]["state"]["tick"], 10);
    }

    #[test]
    fn maps_parabox_actions_with_complete_state() {
        let config = Config {
            source: PathBuf::new(),
            format: SourceFormat::Parabox,
            task_id: "parabox-intro".to_owned(),
            task_label: "Patrick's Parabox".to_owned(),
            listen: String::new(),
        };
        let events = normalize_parabox(
            &config,
            vec![json!({
                "schema": "parabox-events-v1",
                "type": "request",
                "command": "move",
                "argument_count": 1,
                "ok": true,
                "state": {"campaign": {"score": 0}, "level": {"reference": "a1"}},
            })],
        )
        .expect("events");
        assert_eq!(events[0]["sequence"], 1);
        assert_eq!(events[0]["action"]["command"], "move");
        assert_eq!(events[0]["state"]["level"]["reference"], "a1");
    }

    #[test]
    fn rejects_generic_events_without_reconnectable_state() {
        let error = normalize_generic(vec![json!({
            "schema": EVENT_SCHEMA,
            "sequence": 1,
        })])
        .expect_err("state is required");
        assert!(error.contains("invalid event"));
    }
}
