//! Replicate the live read API to Cloudflare (or to a directory for checks).
//!
//! live-publisher --root <repo> --endpoint <https://…> [--token-file <path>]
//! live-publisher --root <repo> --out <directory> [--once]
//!
//! The ingest token is read from --token-file or LIVE_INGEST_TOKEN.

use std::path::PathBuf;
use std::time::Duration;

use benchmark_observer_runtime::publish::{DirectorySink, HttpSink, Publisher, Sink};

fn main() {
    if let Err(error) = run() {
        eprintln!("live-publisher: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let mut root = PathBuf::from(".");
    let mut state = None;
    let mut endpoint = None;
    let mut token_file = None;
    let mut out = None;
    let mut once = false;
    let mut arguments = std::env::args().skip(1);
    while let Some(argument) = arguments.next() {
        let mut value = || arguments.next().ok_or_else(|| format!("{argument} needs a value"));
        match argument.as_str() {
            "--root" => root = PathBuf::from(value()?),
            "--state" => state = Some(PathBuf::from(value()?)),
            "--endpoint" => endpoint = Some(value()?),
            "--token-file" => token_file = Some(PathBuf::from(value()?)),
            "--out" => out = Some(PathBuf::from(value()?)),
            "--once" => once = true,
            other => return Err(format!("unknown argument {other}")),
        }
    }
    let state = state.unwrap_or_else(|| root.join(".harbor/live-publish"));
    match (endpoint, out) {
        (Some(endpoint), None) => {
            let token = match token_file {
                Some(path) => std::fs::read_to_string(&path)
                    .map_err(|error| format!("{}: {error}", path.display()))?,
                None => std::env::var("LIVE_INGEST_TOKEN")
                    .map_err(|_| "set LIVE_INGEST_TOKEN or pass --token-file".to_owned())?,
            };
            serve(Publisher::open(&root, &state, HttpSink::new(&endpoint, token.trim().to_owned()))?, once)
        }
        (None, Some(out)) => serve(Publisher::open(&root, &state, DirectorySink::new(out))?, once),
        _ => Err("pass exactly one of --endpoint or --out".into()),
    }
}

fn serve<S: Sink>(mut publisher: Publisher<S>, once: bool) -> Result<(), String> {
    let mut failures = 0u32;
    loop {
        let result = publisher.cycle().and_then(|_| publisher.heartbeat_if_due());
        match result {
            Ok(()) => failures = 0,
            Err(error) if !once => {
                failures += 1;
                eprintln!("live-publisher: {error}");
            }
            Err(error) => return Err(error),
        }
        if once {
            return Ok(());
        }
        // Back off up to a minute while the remote side is unreachable.
        std::thread::sleep(Duration::from_secs(1u64 << failures.min(6)));
    }
}
