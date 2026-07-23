use std::env;
use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};

use flate2::Compression;
use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use tar::{Archive, Builder, Header};

fn main() {
    if let Err(error) = run() {
        eprintln!("sausage-import-overworld: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let mut arguments = env::args_os().skip(1);
    let entries =
        PathBuf::from(arguments.next().ok_or(
            "usage: sausage-import-overworld ENTRIES.tar.gz SAVES OUTPUT.tar.gz RUN_PREFIX",
        )?);
    let saves = PathBuf::from(arguments.next().ok_or("missing save directory")?);
    let output = PathBuf::from(arguments.next().ok_or("missing output")?);
    let prefix = arguments
        .next()
        .and_then(|value| value.into_string().ok())
        .ok_or("missing run prefix")?;
    if arguments.next().is_some() {
        return Err("too many arguments".into());
    }
    let mut archive = Archive::new(GzDecoder::new(File::open(entries).map_err(display_error)?));
    let mut stems = Vec::new();
    for entry in archive.entries().map_err(display_error)? {
        let entry = entry.map_err(display_error)?;
        if !entry.header().entry_type().is_file() {
            continue;
        }
        let path = entry.path().map_err(display_error)?;
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if let Some(stem) = name.strip_suffix(".state") {
            stems.push(stem.to_owned());
        }
    }
    stems.sort();
    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent).map_err(display_error)?;
    }
    let encoder = GzEncoder::new(
        File::create(output).map_err(display_error)?,
        Compression::default(),
    );
    let mut builder = Builder::new(encoder);
    for stem in stems {
        let (_, level) = stem
            .split_once('-')
            .ok_or_else(|| format!("invalid entry stem {stem}"))?;
        let source = matching_save(&saves, &prefix, level)?;
        let mut bytes = Vec::new();
        File::open(source)
            .map_err(display_error)?
            .read_to_end(&mut bytes)
            .map_err(display_error)?;
        let mut header = Header::new_gnu();
        header.set_size(bytes.len() as u64);
        header.set_mode(0o644);
        header.set_mtime(0);
        header.set_cksum();
        builder
            .append_data(&mut header, format!("{stem}.state"), bytes.as_slice())
            .map_err(display_error)?;
    }
    let encoder = builder.into_inner().map_err(display_error)?;
    encoder.finish().map_err(display_error)?;
    Ok(())
}

fn matching_save(directory: &Path, prefix: &str, level: &str) -> Result<PathBuf, String> {
    let suffix = format!("_{level}.sav");
    let matches = fs::read_dir(directory)
        .map_err(display_error)?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with(prefix) && name.ends_with(&suffix))
        })
        .collect::<Vec<_>>();
    if matches.len() == 1 {
        Ok(matches[0].clone())
    } else {
        Err(format!(
            "expected one save for {level:?}, found {}",
            matches.len()
        ))
    }
}

fn display_error(error: impl std::fmt::Display) -> String {
    error.to_string()
}
