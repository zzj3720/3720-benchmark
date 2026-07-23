use std::env;
use std::io::{self, BufRead, Write};
use std::path::PathBuf;

use benchmark_observer_runtime::{Recorder, Request, Response};

fn main() {
    if let Err(error) = run() {
        eprintln!("run-recorder: {error}");
        std::process::exit(2);
    }
}

fn run() -> Result<(), String> {
    let mut arguments = env::args().skip(1);
    let chain_id = arguments.next().ok_or("missing chain id")?;
    let journal_root = PathBuf::from(arguments.next().ok_or("missing journal root")?);
    if arguments.next().is_some() {
        return Err("unexpected recorder argument".into());
    }
    let mut stdout = io::stdout().lock();
    let mut recorder = match Recorder::open(&chain_id, &journal_root) {
        Ok(recorder) => recorder,
        Err(error) => {
            write_response(&mut stdout, &Response::error(error))?;
            return Ok(());
        }
    };
    let stdin = io::stdin();
    for line in stdin.lock().lines() {
        let line = line.map_err(|error| error.to_string())?;
        let request = match serde_json::from_str::<Request>(&line) {
            Ok(request) => request,
            Err(error) => {
                write_response(&mut stdout, &Response::error(error.to_string()))?;
                continue;
            }
        };
        match recorder.handle(request) {
            Ok(shutdown) => {
                write_response(&mut stdout, &Response::ok())?;
                if shutdown {
                    break;
                }
            }
            Err(error) => write_response(&mut stdout, &Response::error(error))?,
        }
    }
    Ok(())
}

fn write_response(output: &mut impl Write, response: &Response) -> Result<(), String> {
    serde_json::to_writer(&mut *output, response).map_err(|error| error.to_string())?;
    output.write_all(b"\n").map_err(|error| error.to_string())?;
    output.flush().map_err(|error| error.to_string())
}
