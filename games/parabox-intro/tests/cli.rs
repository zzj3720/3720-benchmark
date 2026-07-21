use std::env;
use std::fs;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::sync::Mutex;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use parabox_terminal::state::State;
use serde_json::{Value, json};

const FIRST_LEVEL: &[&str] = &[
    "up", "up", "right", "right", "right", "down", "down", "up", "up", "up", "left", "left",
    "left", "left",
];
const SECOND_LEVEL: &[&str] = &[
    "left", "left", "up", "up", "up", "up", "right", "right", "down", "down", "down", "left",
    "down", "right", "right", "up", "right", "down", "left", "down", "left", "left", "right",
];
static FIXTURE_START: Mutex<()> = Mutex::new(());

struct Fixture {
    root: PathBuf,
    address: String,
    server: Child,
}

impl Fixture {
    fn new() -> Self {
        let start_guard = FIXTURE_START.lock().unwrap();
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = env::temp_dir().join(format!("parabox-cli-{}-{unique}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        let socket = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = socket.local_addr().unwrap().to_string();
        drop(socket);
        let server = Command::new(env!("CARGO_BIN_EXE_parabox-server"))
            .env(
                "PARABOX_CAMPAIGN_DIR",
                Path::new(env!("CARGO_MANIFEST_DIR")).join("data/campaign"),
            )
            .env("PARABOX_STATE", root.join("server-state.txt"))
            .env("PARABOX_AUDIT", root.join("audit.tsv"))
            .env("PARABOX_EVENTS", root.join("events.jsonl"))
            .env("PARABOX_API_RATE_STATE", root.join("rate.txt"))
            .env("PARABOX_LISTEN_ADDR", &address)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let mut ready = false;
        for _ in 0..40 {
            if let Ok(mut stream) = TcpStream::connect(&address) {
                writeln!(stream, "show").unwrap();
                let mut response = String::new();
                stream.read_to_string(&mut response).unwrap();
                if serde_json::from_str::<Value>(&response).is_ok() {
                    ready = true;
                    break;
                }
            }
            thread::sleep(Duration::from_millis(50));
        }
        assert!(ready, "server did not become ready");
        fs::remove_file(root.join("rate.txt")).unwrap();
        fs::write(root.join("audit.tsv"), "parabox-audit-v1\n").unwrap();
        drop(start_guard);
        Self {
            address,
            server,
            root,
        }
    }

    fn command(&self) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_parabox"));
        command.env("PARABOX_API_ADDR", &self.address);
        command
    }

    fn run(&self, args: &[&str]) -> Output {
        self.command().args(args).output().unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.server.kill().unwrap();
        self.server.wait().unwrap();
        fs::remove_dir_all(&self.root).unwrap();
    }
}

fn json_response(output: &Output) -> Value {
    let bytes = if output.stdout.is_empty() {
        &output.stderr
    } else {
        &output.stdout
    };
    let response: Value = serde_json::from_slice(bytes)
        .unwrap_or_else(|error| panic!("response is not JSON: {error}; output={output:?}"));
    assert_eq!(response["api_version"], "parabox-api-v3");
    assert!(response["ok"].is_boolean());
    response
}

fn has_event(response: &Value, kind: &str, level: &str) -> bool {
    response["data"]["events"]
        .as_array()
        .unwrap()
        .iter()
        .any(|event| event["type"] == kind && event["level"] == level)
}

fn wait_for_cooldown() {
    thread::sleep(Duration::from_millis(550));
}

#[test]
fn client_binary_does_not_embed_campaign_data() {
    let client = fs::read(env!("CARGO_BIN_EXE_parabox")).unwrap();
    for secret in [
        b"parabox-complete-364-v11".as_slice(),
        b"parabox-state-v4".as_slice(),
        b"/opt/parabox/campaign".as_slice(),
        b"First Puzzle".as_slice(),
        b"Inpush Player Is Shift".as_slice(),
    ] {
        assert!(
            !client.windows(secret.len()).any(|window| window == secret),
            "thin API client embeds campaign data"
        );
    }
}

#[test]
fn cli_scores_each_solved_level_and_serializes_concurrent_calls() {
    let fixture = Fixture::new();
    let shown = fixture.run(&["show"]);
    assert!(shown.status.success());
    let shown = json_response(&shown);
    assert_eq!(shown["command"], "show");
    let map = shown["data"]["state"]["space"]["map"].as_array().unwrap();
    assert_eq!(map.len(), 7);
    assert!(
        map.iter()
            .all(|row| row.as_array().is_some_and(|cells| cells.len() == 7))
    );
    assert!(shown["data"]["state"].get("observer_scene").is_none());

    let rejected = fixture.run(&["status"]);
    assert_eq!(rejected.status.code(), Some(2));
    let rejected = json_response(&rejected);
    assert!(!rejected["ok"].as_bool().unwrap());
    assert_eq!(rejected["error"]["code"], "rate_limited");
    assert!(
        rejected["error"]["message"]
            .as_str()
            .unwrap()
            .contains("API rate limit")
    );

    wait_for_cooldown();
    let mut args = vec!["move"];
    args.extend_from_slice(FIRST_LEVEL);
    let batch = fixture.run(&args);
    assert!(batch.status.success(), "{batch:?}");
    let batch = json_response(&batch);
    assert!(has_event(&batch, "level_solved", "a1"));
    assert_eq!(batch["data"]["score_delta"], 1);
    assert_eq!(batch["data"]["score"], 1);

    wait_for_cooldown();
    let mut args = vec!["move"];
    args.extend_from_slice(SECOND_LEVEL);
    let batch = fixture.run(&args);
    assert!(batch.status.success(), "{batch:?}");
    let batch = json_response(&batch);
    assert!(has_event(&batch, "level_solved", "a2"));
    assert!(has_event(&batch, "level_unlocked", "a3"));
    assert_eq!(batch["data"]["score_delta"], 1);
    assert_eq!(batch["data"]["score"], 2);
    assert_eq!(
        batch["data"]["state"]["level"]["reference"],
        Value::String("a3".to_string())
    );

    wait_for_cooldown();
    let status = fixture.run(&["status"]);
    assert!(status.status.success());
    let status = json_response(&status);
    assert_eq!(status["data"]["solved"], 2);
    assert_eq!(status["data"]["total"], 364);
    assert_eq!(status["data"]["current"], "a3");

    wait_for_cooldown();
    let submission = fixture.run(&["submit"]);
    assert!(submission.status.success());
    let submission = json_response(&submission);
    assert_eq!(submission["data"]["score"], 2);
    assert_eq!(submission["data"]["total"], 364);
    assert_eq!(submission["data"]["recorded"], true);

    wait_for_cooldown();
    let mut first = fixture.command();
    first
        .arg("levels")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut second = fixture.command();
    second
        .arg("levels")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let first = first.spawn().unwrap();
    let second = second.spawn().unwrap();
    let first = first.wait_with_output().unwrap();
    let second = second.wait_with_output().unwrap();
    let codes = [first.status.code(), second.status.code()];
    assert!(
        matches!(codes, [Some(0), Some(2)] | [Some(2), Some(0)]),
        "expected one accepted and one rate-limited call, got {codes:?}"
    );
    assert!(json_response(&first)["ok"].is_boolean());
    assert!(json_response(&second)["ok"].is_boolean());

    let audit = fs::read_to_string(fixture.root.join("audit.tsv")).unwrap();
    assert!(audit.starts_with("parabox-audit-v1\n"));
    assert!(audit.contains("\t0\tmove "));

    let events: Vec<Value> = fs::read_to_string(fixture.root.join("events.jsonl"))
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(events[0]["schema"], "parabox-events-v1");
    assert_eq!(events[0]["type"], "sidecar_started");
    assert_eq!(events[0]["scene"]["schema"], "parabox-observer-scene-v1");
    assert!(
        events[0]["scene"]["spaces"]
            .as_array()
            .is_some_and(|spaces| !spaces.is_empty())
    );
    assert!(events[0]["scene"]["spaces"][0]["map"][0].is_string());
    assert!(events.iter().any(|event| {
        event["type"] == "request"
            && event["command"] == "move"
            && event["score_before"] == 0
            && event["score"] == 1
            && event["score_delta"] == 1
            && event["selected_before"] == "a1"
            && event["selected"] == "a2"
            && event["solved_levels"] == json!(["a1"])
    }));
    assert!(events.windows(2).all(|pair| {
        pair[0]["timestamp_ms"].as_u64().unwrap() <= pair[1]["timestamp_ms"].as_u64().unwrap()
    }));
}

#[test]
fn move_batch_is_capped_by_the_server() {
    let fixture = Fixture::new();
    let mut args = vec!["move"];
    args.extend(std::iter::repeat_n("up", 33));
    let rejected = fixture.run(&args);
    assert_eq!(rejected.status.code(), Some(2));
    let rejected = json_response(&rejected);
    assert_eq!(rejected["error"]["code"], "too_many_moves");
    assert!(
        rejected["error"]["message"]
            .as_str()
            .unwrap()
            .contains("at most 32")
    );

    let audit = fs::read_to_string(fixture.root.join("audit.tsv")).unwrap();
    assert!(audit.contains("\t2\tmove "));
}

#[test]
fn a_level_accepts_more_than_256_directions() {
    let fixture = Fixture::new();
    for batch_index in 0..9 {
        if batch_index > 0 {
            wait_for_cooldown();
        }
        let mut args = vec!["move"];
        args.extend(std::iter::repeat_n("left", 32));
        let accepted = fixture.run(&args);
        assert!(accepted.status.success(), "{accepted:?}");
    }

    let state = State::load(&fixture.root.join("server-state.txt")).unwrap();
    assert_eq!(state.histories[0].len(), 288);
}

#[test]
fn raw_api_response_is_one_json_document() {
    let fixture = Fixture::new();
    let mut stream = (0..40)
        .find_map(|_| match TcpStream::connect(&fixture.address) {
            Ok(stream) => Some(stream),
            Err(_) => {
                thread::sleep(Duration::from_millis(50));
                None
            }
        })
        .expect("server did not become ready");
    writeln!(stream, "show").unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    assert!(response.ends_with('\n'));
    assert_eq!(response.lines().count(), 1);
    let response: Value = serde_json::from_str(&response).unwrap();
    assert_eq!(response["api_version"], "parabox-api-v3");
    assert_eq!(response["ok"], true);
    assert_eq!(response["command"], "show");
}

#[test]
fn every_agent_command_returns_json_and_inspect_uses_a_2d_map() {
    let fixture = Fixture::new();
    for arguments in [
        &["help"][..],
        &["show"][..],
        &["inspect", "3", "5"][..],
        &["move", "up"][..],
        &["undo"][..],
        &["restart"][..],
        &["levels"][..],
        &["status"][..],
        &["submit"][..],
    ] {
        if arguments != ["help"] {
            wait_for_cooldown();
        }
        let output = fixture.run(arguments);
        assert!(output.status.success(), "{arguments:?}: {output:?}");
        let response = json_response(&output);
        assert_eq!(response["ok"], true);
        assert_eq!(response["command"], arguments[0]);
        assert_eq!(
            response["data"]["score"]
                .as_u64()
                .or_else(|| response["data"]["state"]["campaign"]["score"].as_u64()),
            Some(0),
            "{arguments:?} must report the current integer score"
        );
        if arguments[0] == "inspect" {
            let space = &response["data"]["space"];
            assert_eq!(space["row_order"], "top_to_bottom");
            assert_eq!(space["column_order"], "left_to_right");
            let map = space["map"].as_array().unwrap();
            assert_eq!(map.len(), space["height"].as_u64().unwrap() as usize);
            assert!(map.iter().all(|row| {
                row.as_array().is_some_and(|cells| {
                    cells.len() == space["width"].as_u64().unwrap() as usize
                        && cells.iter().all(|cell| {
                            cell.as_str()
                                .is_some_and(|symbol| symbol.chars().count() == 1)
                        })
                })
            }));
        }
    }
}
