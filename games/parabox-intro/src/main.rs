use std::env;
use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use parabox_terminal::campaign::{
    CAMPAIGN_ID, LEVELS, area, immediate_successor, level_available, level_index,
};
use parabox_terminal::engine::{Direction, Game, load_level};
use parabox_terminal::rate_limit;
use parabox_terminal::state::State;
use serde_json::{Value, json};

const AUDIT_HEADER: &str = "parabox-audit-v1";
const EVENT_SCHEMA: &str = "parabox-events-v1";
const API_VERSION: &str = "parabox-api-v3";
const MAX_MOVES_PER_CALL: usize = 32;

struct Progress {
    games: Vec<Option<Game>>,
    solved: Vec<bool>,
    available: Vec<bool>,
    current: Option<usize>,
}

fn replay_campaign(campaign_dir: &Path, state: &State) -> Result<Progress, String> {
    let mut solved = vec![false; LEVELS.len()];
    for &index in &state.solved_order {
        if index >= LEVELS.len() || solved[index] {
            return Err("state has an invalid solved order".to_string());
        }
        if !level_available(index, &solved) {
            return Err(format!(
                "level {} was solved before it was available",
                LEVELS[index].reference
            ));
        }
        solved[index] = true;
    }

    let mut games = Vec::with_capacity(LEVELS.len());
    for (index, entry) in LEVELS.iter().enumerate() {
        let should_replay =
            solved[index] || state.selected == Some(index) || !state.histories[index].is_empty();
        if should_replay {
            let mut game = load_level(campaign_dir.join("levels").join(entry.file))?;
            for &action in &state.histories[index] {
                game.move_player(action);
            }
            if game.won() != solved[index] {
                return Err(format!(
                    "level {} history and solved order disagree",
                    entry.reference
                ));
            }
            games.push(Some(game));
        } else {
            games.push(None);
        }
    }

    let available: Vec<_> = (0..LEVELS.len())
        .map(|index| level_available(index, &solved))
        .collect();
    if let Some(index) = state.selected {
        if solved[index] {
            return Err(format!(
                "selected level {} is already solved",
                LEVELS[index].reference
            ));
        }
        if !available[index] {
            return Err(format!(
                "selected level {} is locked",
                LEVELS[index].reference
            ));
        }
    }

    Ok(Progress {
        games,
        solved,
        available,
        current: state.selected,
    })
}

fn game_state(progress: &Progress) -> Result<Value, String> {
    let score = progress.solved.iter().filter(|&&won| won).count();
    let (status, level, space) = match progress.current {
        Some(index) => {
            let entry = LEVELS[index];
            (
                "in_progress",
                json!({
                    "number": index + 1,
                    "total": LEVELS.len(),
                    "reference": entry.reference,
                    "title": entry.title,
                    "area": area(entry.area).name,
                    "kind": entry.kind.as_str(),
                    "optional": entry.kind.as_str() != "core",
                }),
                serde_json::to_value(
                    progress.games[index]
                        .as_ref()
                        .ok_or_else(|| "selected level was not loaded".to_string())?
                        .current_space()?,
                )
                .map_err(|error| format!("failed to serialize game state: {error}"))?,
            )
        }
        None if score == LEVELS.len() => ("complete", Value::Null, Value::Null),
        None => ("selection_required", Value::Null, Value::Null),
    };
    let choices: Vec<_> = LEVELS
        .iter()
        .enumerate()
        .filter(|(index, _)| progress.available[*index] && !progress.solved[*index])
        .map(|(_, entry)| {
            json!({
                "reference": entry.reference,
                "title": entry.title,
                "area": area(entry.area).name,
                "kind": entry.kind.as_str(),
                "optional": entry.kind.as_str() != "core",
            })
        })
        .collect();
    Ok(json!({
        "campaign": {
            "id": CAMPAIGN_ID,
            "score": score,
            "solved": score,
            "total": LEVELS.len(),
            "complete": score == LEVELS.len(),
        },
        "status": status,
        "level": level,
        "available_levels": choices,
        "space": space,
        "symbols": {
            "@": "player",
            "0-9": "box",
            "#": "wall",
            ".": "box_goal",
            "+": "player_goal",
            "X": "box_on_goal",
            "P": "player_on_goal",
            " ": "empty",
        },
    }))
}

fn level_list(progress: &Progress) -> Value {
    Value::Array(
        LEVELS
            .iter()
            .enumerate()
            .map(|(index, entry)| {
                let status = if progress.solved[index] {
                    "solved"
                } else if progress.current == Some(index) {
                    "current"
                } else if progress.available[index] {
                    "available"
                } else {
                    "locked"
                };
                json!({
                    "number": index + 1,
                    "reference": entry.reference,
                    "title": entry.title,
                    "area": area(entry.area).name,
                    "kind": entry.kind.as_str(),
                    "optional": entry.kind.as_str() != "core",
                    "predecessor": entry.predecessor,
                    "immediate_from_predecessor": entry.immediate,
                    "status": status,
                })
            })
            .collect(),
    )
}

fn api_rate_path() -> PathBuf {
    #[cfg(debug_assertions)]
    if let Ok(path) = env::var("PARABOX_API_RATE_STATE") {
        return PathBuf::from(path);
    }
    PathBuf::from("/var/tmp/parabox-api-rate.txt")
}

fn audit_path() -> PathBuf {
    PathBuf::from(
        env::var("PARABOX_AUDIT").unwrap_or_else(|_| "/var/lib/parabox/parabox-audit.tsv".into()),
    )
}

fn event_path() -> PathBuf {
    PathBuf::from(
        env::var("PARABOX_EVENTS")
            .unwrap_or_else(|_| "/var/lib/parabox/parabox-events.jsonl".into()),
    )
}

fn state_path() -> PathBuf {
    PathBuf::from(
        env::var("PARABOX_STATE").unwrap_or_else(|_| "/var/lib/parabox/parabox-state.txt".into()),
    )
}

fn campaign_dir() -> PathBuf {
    PathBuf::from(
        env::var("PARABOX_CAMPAIGN_DIR").unwrap_or_else(|_| "/opt/parabox/campaign".into()),
    )
}

fn timestamp_ms() -> Result<u64, String> {
    let milliseconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| format!("system clock is before the Unix epoch: {error}"))?
        .as_millis();
    u64::try_from(milliseconds).map_err(|_| "system timestamp does not fit in u64".to_string())
}

fn append_audit(arguments: &[String], code: i32, timestamp_ms: u64) -> Result<(), String> {
    let path = audit_path();
    let mut file = OpenOptions::new()
        .append(true)
        .open(&path)
        .map_err(|error| format!("failed to open {}: {error}", path.display()))?;
    writeln!(file, "{timestamp_ms}\t{code}\t{}", arguments.join(" "))
        .map_err(|error| format!("failed to append {}: {error}", path.display()))
}

fn append_event(record: &Value) -> Result<(), String> {
    let path = event_path();
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|error| format!("failed to open {}: {error}", path.display()))?;
    serde_json::to_writer(&mut file, record)
        .map_err(|error| format!("failed to append {}: {error}", path.display()))?;
    file.write_all(b"\n")
        .map_err(|error| format!("failed to append {}: {error}", path.display()))
}

fn state_record(record_type: &str, timestamp_ms: u64) -> Result<Value, String> {
    let state = State::load(&state_path())?;
    let progress = replay_campaign(&campaign_dir(), &state)?;
    Ok(json!({
        "schema": EVENT_SCHEMA,
        "type": record_type,
        "timestamp_ms": timestamp_ms,
        "campaign": CAMPAIGN_ID,
        "score": state.solved_order.len(),
        "total": LEVELS.len(),
        "selected": state.selected.map(|index| LEVELS[index].reference),
        "state": game_state(&progress)?,
    }))
}

fn append_request_event(
    arguments: &[String],
    code: i32,
    response: &Value,
    state_before: &State,
    timestamp_ms: u64,
) -> Result<(), String> {
    let mut record = state_record("request", timestamp_ms)?;
    let solved_levels: Vec<_> = response
        .pointer("/data/events")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|event| event.get("type").and_then(Value::as_str) == Some("level_solved"))
        .filter_map(|event| event.get("level").and_then(Value::as_str))
        .collect();
    record["ok"] = json!(code == 0);
    record["code"] = json!(code);
    record["command"] = json!(arguments.first().map(String::as_str).unwrap_or("show"));
    record["argument_count"] = json!(arguments.len().saturating_sub(1));
    record["score_before"] = json!(state_before.solved_order.len());
    record["selected_before"] = json!(state_before.selected.map(|index| LEVELS[index].reference));
    record["score_delta"] = json!(
        record["score"]
            .as_u64()
            .unwrap_or(0)
            .saturating_sub(state_before.solved_order.len() as u64)
    );
    record["solved_levels"] = json!(solved_levels);
    if let Some(state) = response.pointer("/data/state") {
        record["state"] = state.clone();
    }
    append_event(&record)
}

fn execute(arguments: &[String]) -> Result<Value, String> {
    let campaign_dir = campaign_dir();
    let state_path = state_path();
    let rate_path = api_rate_path();
    let command = arguments.first().map(String::as_str).unwrap_or("show");
    rate_limit::enforce(&rate_path)?;
    let mut state = State::load(&state_path)?;
    let mut progress = replay_campaign(&campaign_dir, &state)?;

    match command {
        "show" => {
            state.save(&state_path)?;
            Ok(json!({"state": game_state(&progress)?}))
        }
        "status" => Ok(json!({
            "campaign": CAMPAIGN_ID,
            "score": progress.solved.iter().filter(|&&won| won).count(),
            "solved": progress.solved.iter().filter(|&&won| won).count(),
            "total": LEVELS.len(),
            "current": progress.current.map(|index| LEVELS[index].reference),
            "available": LEVELS.iter().enumerate()
                .filter(|(index, _)| progress.available[*index] && !progress.solved[*index])
                .map(|(_, entry)| entry.reference)
                .collect::<Vec<_>>(),
            "complete": progress.solved.iter().all(|&won| won),
        })),
        "submit" => {
            let score = progress.solved.iter().filter(|&&won| won).count();
            Ok(json!({
                "score": score,
                "total": LEVELS.len(),
                "current": progress.current.map(|index| LEVELS[index].reference),
                "complete": score == LEVELS.len(),
                "recorded": true,
            }))
        }
        "levels" => {
            let score = progress.solved.iter().filter(|&&won| won).count();
            Ok(json!({
                "score": score,
                "total": LEVELS.len(),
                "scoring": "1 point per solved puzzle",
                "levels": level_list(&progress),
            }))
        }
        "select" => {
            let reference = arguments
                .get(1)
                .ok_or_else(|| "select requires a level reference".to_string())?;
            if arguments.len() != 2 {
                return Err("select accepts exactly one level reference".to_string());
            }
            let index = level_index(reference)
                .ok_or_else(|| format!("unknown level reference: {reference}"))?;
            if progress.solved[index] {
                return Err(format!("level {reference} is already solved"));
            }
            if !progress.available[index] {
                return Err(format!("level {reference} is locked"));
            }
            state.selected = Some(index);
            state.save(&state_path)?;
            progress = replay_campaign(&campaign_dir, &state)?;
            Ok(json!({"state": game_state(&progress)?}))
        }
        "move" => {
            let actions = arguments
                .iter()
                .skip(1)
                .map(|value| Direction::parse(value))
                .collect::<Result<Vec<_>, _>>()?;
            if actions.is_empty() {
                return Err("move requires at least one direction".to_string());
            }

            let mut events = Vec::new();
            let index = progress.current.ok_or_else(|| {
                "no level is selected; use levels and select <reference>".to_string()
            })?;
            let available_before = progress.available.clone();
            let action_count = actions.len();
            let mut applied = 0;
            for action in actions {
                state.histories[index].push(action);
                progress.games[index]
                    .as_mut()
                    .ok_or_else(|| "selected level was not loaded".to_string())?
                    .move_player(action);
                applied += 1;
                if progress.games[index].as_ref().is_some_and(Game::won) {
                    state.solved_order.push(index);
                    let mut solved_after = progress.solved.clone();
                    solved_after[index] = true;
                    state.selected = immediate_successor(index).filter(|&next| {
                        !solved_after[next] && level_available(next, &solved_after)
                    });
                    events.push(json!({
                        "type": "level_solved",
                        "level": LEVELS[index].reference,
                        "score_delta": 1,
                        "score": state.solved_order.len(),
                    }));
                    progress = replay_campaign(&campaign_dir, &state)?;
                    for next_index in 0..LEVELS.len() {
                        if !available_before[next_index]
                            && progress.available[next_index]
                            && !progress.solved[next_index]
                        {
                            events.push(json!({
                                "type": "level_unlocked",
                                "level": LEVELS[next_index].reference,
                                "kind": LEVELS[next_index].kind.as_str(),
                                "optional": LEVELS[next_index].kind.as_str() != "core",
                            }));
                        }
                    }
                    if let Some(next_index) = state.selected {
                        events.push(json!({
                            "type": "level_selected",
                            "level": LEVELS[next_index].reference,
                        }));
                    }
                    break;
                }
            }
            state.save(&state_path)?;
            Ok(json!({
                "events": events,
                "moves_applied": applied,
                "unused_moves": action_count - applied,
                "score_delta": usize::from(progress.solved[index]),
                "score": progress.solved.iter().filter(|&&won| won).count(),
                "state": game_state(&progress)?,
            }))
        }
        "undo" => {
            let count = match arguments.get(1) {
                Some(value) => value
                    .parse::<usize>()
                    .map_err(|_| "undo count must be an integer".to_string())?,
                None => 1,
            };
            if arguments.len() > 2 || count == 0 || count > MAX_MOVES_PER_CALL {
                return Err(format!(
                    "undo accepts an optional count from 1 to {MAX_MOVES_PER_CALL}"
                ));
            }
            let index = progress.current.ok_or_else(|| {
                "no level is selected; use levels and select <reference>".to_string()
            })?;
            let before = state.histories[index].len();
            state.histories[index].truncate(before.saturating_sub(count));
            let undone = before - state.histories[index].len();
            state.save(&state_path)?;
            progress = replay_campaign(&campaign_dir, &state)?;
            Ok(json!({"undone": undone, "state": game_state(&progress)?}))
        }
        "restart" => {
            let index = progress.current.ok_or_else(|| {
                "no level is selected; use levels and select <reference>".to_string()
            })?;
            state.histories[index].clear();
            state.save(&state_path)?;
            progress = replay_campaign(&campaign_dir, &state)?;
            Ok(json!({"state": game_state(&progress)?}))
        }
        "inspect" => {
            let row = arguments
                .get(1)
                .ok_or_else(|| "inspect requires a row and column".to_string())?
                .parse()
                .map_err(|_| "inspect row must be a non-negative integer".to_string())?;
            let column = arguments
                .get(2)
                .ok_or_else(|| "inspect requires a row and column".to_string())?
                .parse()
                .map_err(|_| "inspect column must be a non-negative integer".to_string())?;
            if arguments.len() != 3 {
                return Err("inspect accepts exactly one row and one column".to_string());
            }
            let index = progress
                .current
                .ok_or_else(|| "no level is selected".to_string())?;
            let (box_id, space) = progress.games[index]
                .as_ref()
                .ok_or_else(|| "selected level was not loaded".to_string())?
                .inspect_at(row, column)?;
            Ok(json!({
                "position": {"row": row, "column": column},
                "box_id": box_id,
                "space": space,
                "score": progress.solved.iter().filter(|&&won| won).count(),
                "total": LEVELS.len(),
            }))
        }
        "help" | "--help" | "-h" => {
            let score = progress.solved.iter().filter(|&&won| won).count();
            Ok(json!({
                "usage": "parabox [show|status|submit|levels|select REF|move DIR...|undo [COUNT]|restart|inspect ROW COLUMN]",
                "commands": ["show", "status", "submit", "levels", "select", "move", "undo", "restart", "inspect"],
                "directions": ["up", "down", "left", "right"],
                "cooldown_ms": 500,
                "max_moves_per_call": MAX_MOVES_PER_CALL,
                "score": score,
                "total": LEVELS.len(),
                "scoring": "1 point per solved puzzle; points are never converted to a percentage",
                "map": {
                    "type": "two_dimensional_character_array",
                    "row_order": "top_to_bottom",
                    "column_order": "left_to_right",
                },
            }))
        }
        _ => Err(format!("unknown command: {command}")),
    }
}

fn success_response(command: &str, data: Value) -> Value {
    json!({
        "api_version": API_VERSION,
        "ok": true,
        "command": command,
        "data": data,
    })
}

fn error_response(command: &str, code: &str, message: impl Into<String>) -> Value {
    json!({
        "api_version": API_VERSION,
        "ok": false,
        "command": command,
        "error": {
            "code": code,
            "message": message.into(),
        },
    })
}

fn handle(mut stream: TcpStream) -> Result<(), String> {
    let mut request = String::new();
    BufReader::new(
        stream
            .try_clone()
            .map_err(|error| format!("failed to clone API connection: {error}"))?,
    )
    .take(65_537)
    .read_line(&mut request)
    .map_err(|error| format!("failed to read API request: {error}"))?;
    let request_too_large = request.len() > 65_536;
    let arguments: Vec<_> = if request_too_large {
        Vec::new()
    } else {
        request
            .split_whitespace()
            .map(ToString::to_string)
            .collect()
    };
    let command = arguments.first().map(String::as_str).unwrap_or("show");
    let invalid_arguments = arguments.iter().any(|argument| {
        argument
            .chars()
            .any(|ch| !ch.is_ascii_alphanumeric() && ch != '-')
    });
    let state_before = State::load(&state_path())?;

    let (code, response) = if request_too_large {
        (
            2,
            error_response(command, "request_too_large", "API request is too large"),
        )
    } else if invalid_arguments {
        (
            2,
            error_response(
                command,
                "invalid_request",
                "API request contains an invalid argument",
            ),
        )
    } else if command == "move" && arguments.len().saturating_sub(1) > MAX_MOVES_PER_CALL {
        (
            2,
            error_response(
                command,
                "too_many_moves",
                format!("move accepts at most {MAX_MOVES_PER_CALL} directions"),
            ),
        )
    } else {
        match execute(&arguments) {
            Ok(data) => (0, success_response(command, data)),
            Err(error) => {
                let error_code = if error.starts_with("API rate limit") {
                    "rate_limited"
                } else {
                    "command_failed"
                };
                (2, error_response(command, error_code, error))
            }
        }
    };
    let timestamp_ms = timestamp_ms()?;
    append_audit(&arguments, code, timestamp_ms)?;
    append_request_event(&arguments, code, &response, &state_before, timestamp_ms)?;
    serde_json::to_writer(&mut stream, &response)
        .map_err(|error| format!("failed to serialize API response: {error}"))?;
    stream
        .write_all(b"\n")
        .map_err(|error| format!("failed to write API response: {error}"))
}

fn serve() -> Result<(), String> {
    let state_path = state_path();
    if !state_path.exists() {
        State::new().save(&state_path)?;
    }
    let audit_path = audit_path();
    if !audit_path.exists() {
        if let Some(parent) = audit_path.parent() {
            fs::create_dir_all(parent)
                .map_err(|error| format!("failed to create {}: {error}", parent.display()))?;
        }
        fs::write(&audit_path, format!("{AUDIT_HEADER}\n"))
            .map_err(|error| format!("failed to initialize {}: {error}", audit_path.display()))?;
    }
    let event_path = event_path();
    if let Some(parent) = event_path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("failed to create {}: {error}", parent.display()))?;
    }
    append_event(&state_record("sidecar_started", timestamp_ms()?)?)?;
    let rate_path = api_rate_path();
    if rate_path.exists() {
        fs::remove_file(&rate_path)
            .map_err(|error| format!("failed to reset API cooldown: {error}"))?;
    }
    let address = env::var("PARABOX_LISTEN_ADDR").unwrap_or_else(|_| "0.0.0.0:3720".to_string());
    let listener = TcpListener::bind(&address)
        .map_err(|error| format!("failed to listen on {address}: {error}"))?;
    for connection in listener.incoming() {
        match connection {
            Ok(stream) => {
                if let Err(error) = handle(stream) {
                    eprintln!("parabox-server: {error}");
                }
            }
            Err(error) => eprintln!("parabox-server: failed connection: {error}"),
        }
    }
    Ok(())
}

fn main() {
    if let Err(error) = serve() {
        eprintln!("parabox-server: {error}");
        std::process::exit(2);
    }
}
