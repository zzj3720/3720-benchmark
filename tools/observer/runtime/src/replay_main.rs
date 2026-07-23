use std::io::{self, Read};

use benchmark_observer_runtime::replay_projection;
use serde::Deserialize;
use serde_json::Value;

#[derive(Deserialize)]
struct Request {
    events: Vec<Value>,
    start: usize,
    end: usize,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("replay-projector: {error}");
        std::process::exit(2);
    }
}

fn run() -> Result<(), String> {
    let mut input = String::new();
    io::stdin()
        .read_to_string(&mut input)
        .map_err(|error| error.to_string())?;
    let request: Request = serde_json::from_str(&input).map_err(|error| error.to_string())?;
    let projection = replay_projection(&request.events, request.start, request.end)?;
    serde_json::to_writer(io::stdout().lock(), &projection).map_err(|error| error.to_string())
}
