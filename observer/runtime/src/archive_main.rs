use std::path::PathBuf;
fn main() {
    let mut root = PathBuf::from(".");
    let mut apply = false;
    let mut retire = false;
    let mut chunk_mib = 16;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--cat" => {
                let path = PathBuf::from(args.next().expect("--cat path"));
                let reader = if path.is_dir() {
                    benchmark_observer_runtime::storage::journal_reader(&path)
                } else {
                    benchmark_observer_runtime::storage::reader(&path)
                };
                match reader.and_then(|mut reader| {
                    std::io::copy(&mut reader, &mut std::io::stdout().lock())
                        .map_err(|error| error.to_string())
                }) {
                    Ok(_) => return,
                    Err(error) => {
                        eprintln!("{error}");
                        std::process::exit(1)
                    }
                }
            }
            "--root" => root = PathBuf::from(args.next().expect("--root value")),
            "--chunk-mib" => {
                chunk_mib = args
                    .next()
                    .expect("--chunk-mib value")
                    .parse::<usize>()
                    .expect("chunk MiB");
                assert!(
                    matches!(chunk_mib, 16 | 32),
                    "chunk size must be 16 or 32 MiB"
                );
            }
            "--apply" => apply = true,
            "--retire" => retire = true,
            _ => {
                eprintln!("usage: run-archive --root REPO [--apply [--retire]]");
                std::process::exit(2)
            }
        }
    }
    match benchmark_observer_runtime::archive::migrate_with_chunks(
        &root,
        apply,
        retire,
        chunk_mib * 1024 * 1024,
    ) {
        Ok(value) => println!("{}", serde_json::to_string_pretty(&value).unwrap()),
        Err(error) => {
            eprintln!("archive migration failed: {error}");
            std::process::exit(1)
        }
    }
}
