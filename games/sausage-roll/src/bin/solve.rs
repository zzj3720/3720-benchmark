use std::env;
use std::fs;
use std::process::Command;

use serde_json::Value;

fn main() {
    let walkthrough = env::args().nth(1).expect("usage: sausage-solve <all.dem>");
    let client = env::var("SAUSAGE_BIN").unwrap_or_else(|_| "/usr/local/bin/sausage".into());
    let mut directions = Vec::new();
    for line in fs::read_to_string(walkthrough)
        .expect("walkthrough")
        .lines()
    {
        let direction = line.trim().to_ascii_lowercase();
        if direction.is_empty() {
            continue;
        }
        if direction == "undo" {
            directions.pop();
        } else {
            directions.push(direction);
        }
    }
    let mut cursor = 0;
    while cursor < directions.len() {
        let mut command = Command::new(&client);
        command
            .arg("move")
            .args(&directions[cursor..directions.len().min(cursor + 32)]);
        let output = command.output().expect("sausage client");
        if !output.status.success() {
            eprint!("{}", String::from_utf8_lossy(&output.stderr));
            std::process::exit(2);
        }
        let response: Value = serde_json::from_slice(&output.stdout).expect("client response");
        let applied = response
            .pointer("/data/applied")
            .and_then(Value::as_u64)
            .unwrap_or(0);
        if applied == 0 {
            panic!("walkthrough made no progress at action {}", cursor + 1);
        }
        cursor += applied as usize;
    }
    let output = Command::new(&client)
        .arg("submit")
        .output()
        .expect("sausage submit");
    let response: Value = serde_json::from_slice(&output.stdout).expect("submit response");
    assert_eq!(
        response.pointer("/data/score").and_then(Value::as_u64),
        Some(86)
    );
    assert_eq!(
        response.pointer("/data/total").and_then(Value::as_u64),
        Some(86)
    );
    assert_eq!(
        response.pointer("/data/complete").and_then(Value::as_bool),
        Some(true)
    );
    println!(
        "{}",
        serde_json::to_string(&response).expect("response JSON")
    );
}
