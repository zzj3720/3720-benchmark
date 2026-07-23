use std::path::PathBuf;

use benchmark_observer_runtime::gateway;

#[tokio::main]
async fn main() {
    let mut root = std::env::current_dir().expect("current directory");
    let mut host = std::env::var("LIVE_GATEWAY_HOST").unwrap_or_else(|_| "127.0.0.1".into());
    let mut port = std::env::var("LIVE_GATEWAY_PORT")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(3740);
    let mut arguments = std::env::args().skip(1);
    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "--root" => root = PathBuf::from(arguments.next().expect("--root value")),
            "--host" => host = arguments.next().expect("--host value"),
            "--port" => {
                port = arguments
                    .next()
                    .expect("--port value")
                    .parse()
                    .expect("port")
            }
            _ => panic!("unknown argument {argument}"),
        }
    }
    if let Err(error) = gateway::serve(root, &host, port).await {
        eprintln!("live gateway failed: {error}");
        std::process::exit(1);
    }
}
