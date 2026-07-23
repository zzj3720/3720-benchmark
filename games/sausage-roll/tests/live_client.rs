use std::fs;
use std::net::TcpListener;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

struct Server(Child);

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
fn packaged_client_surface_reaches_the_authoritative_server() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let run = std::env::temp_dir().join(format!(
        "sausage-client-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&run).unwrap();
    let inbox = run.join("game-inbox.jsonl");
    fs::File::create(&inbox).unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    let child = Command::new(env!("CARGO_BIN_EXE_sausage-server"))
        .env(
            "SAUSAGE_CAMPAIGN",
            root.join("data/campaign/merged_binary.gz"),
        )
        .env("SAUSAGE_ENTRIES", root.join("data/campaign/entries.tar.gz"))
        .env("SAUSAGE_STATE", run.join("session.json"))
        .env("SAUSAGE_AUDIT", run.join("audit.jsonl"))
        .env("SAUSAGE_EVENTS", run.join("events.jsonl"))
        .env("BENCHMARK_OBSERVER_INBOX", &inbox)
        .env("SAUSAGE_LISTEN_ADDR", format!("127.0.0.1:{port}"))
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let server = Server(child);
    let output = Command::new(env!("CARGO_BIN_EXE_sausage"))
        .arg("status")
        .env("SAUSAGE_URL", format!("http://127.0.0.1:{port}"))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "client failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let response: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(response["ok"], true);
    assert_eq!(response["command"], "show");
    assert!(
        output.stdout.len() < 35_000,
        "{} bytes",
        output.stdout.len()
    );
    assert!(
        response["data"]["overworld"]["entrances"]
            .as_array()
            .unwrap()
            .len()
            < 86
    );
    let observer_events = fs::read_to_string(&inbox).unwrap();
    let command_event: serde_json::Value =
        serde_json::from_str(observer_events.lines().last().unwrap()).unwrap();
    assert_eq!(
        command_event["state"]["overworld"]["entrances"]
            .as_array()
            .unwrap()
            .len(),
        86
    );
    drop(server);
    fs::remove_dir_all(run).unwrap();
}
