use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

use regex::Regex;

const DEFAULT_URL: &str = "https://steamcommunity.com/sharedfiles/filedetails/?id=2786724419";
const WORLDS: &[(&str, &str, usize)] = &[
    ("Intro", "a", 9),
    ("Enter", "b", 18),
    ("Empty", "c", 14),
    ("Eat", "d", 13),
    ("Reference", "e", 12),
    ("Swap", "L", 5),
    ("Center", "f", 16),
    ("Clone", "g", 25),
    ("Transfer", "h", 29),
    ("Open", "i", 12),
    ("Flip", "j", 17),
    ("Cycle", "k", 18),
    ("Player", "m", 24),
    ("Possess", "n", 22),
    ("Wall", "o", 15),
    ("Infinite Exit", "p", 18),
    ("Infinite Enter", "q", 20),
    ("Multi Infinite", "r", 11),
    ("Challenge", "s", 38),
    ("Gallery", "t", 3),
    ("Appendix: Priority", "u", 9),
    ("Appendix: Extrude", "v", 8),
    ("Appendix: Inner Push", "w", 8),
];

fn main() {
    if let Err(error) = run() {
        eprintln!("parabox-import-walkthrough: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let mut url = DEFAULT_URL.to_owned();
    let mut output = None;
    let mut args = env::args().skip(1);
    while let Some(argument) = args.next() {
        match argument.as_str() {
            "--url" => url = args.next().ok_or("--url requires a value")?,
            "--output" => output = Some(PathBuf::from(args.next().ok_or("--output value")?)),
            _ => return Err(format!("unknown argument {argument}")),
        }
    }
    let output = output.ok_or("usage: parabox-import-walkthrough [--url URL] --output FILE")?;
    let markdown = read_markdown(&url)?;
    let sections = sections(&markdown)?;
    let mut rows = Vec::new();
    for (world, prefix, count) in WORLDS {
        let items = numbered_items(
            sections
                .get(*world)
                .ok_or_else(|| format!("walkthrough has no {world} section"))?,
        )?;
        for number in 1..=*count {
            let item = items
                .get(&number)
                .ok_or_else(|| format!("walkthrough has no {world} level {number}"))?;
            rows.push(format!(
                "{prefix}{number}\t{}",
                directions(item, world, number)?
            ));
        }
    }
    if rows.len() != 364 {
        return Err(format!("expected 364 oracle traces, found {}", rows.len()));
    }
    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent).map_err(display_error)?;
    }
    fs::write(
        output,
        format!(
            "# source: {url}\n# imported direction traces; never package this file in the agent image\n{}\n",
            rows.join("\n")
        ),
    )
    .map_err(display_error)
}

fn read_markdown(url: &str) -> Result<String, String> {
    if let Some(path) = url.strip_prefix("file://") {
        return fs::read_to_string(path).map_err(display_error);
    }
    let reader = format!(
        "https://r.jina.ai/http://{}",
        url.trim_start_matches("https://")
            .trim_start_matches("http://")
    );
    let output = Command::new("curl")
        .args(["--fail", "--silent", "--show-error", "--location", &reader])
        .output()
        .map_err(display_error)?;
    if !output.status.success() {
        return Err(format!("curl exited with {}", output.status));
    }
    String::from_utf8(output.stdout).map_err(display_error)
}

fn sections(markdown: &str) -> Result<HashMap<String, String>, String> {
    let lines = markdown.lines().collect::<Vec<_>>();
    let mut positions = HashMap::new();
    for (index, line) in lines.iter().enumerate() {
        let heading = line.trim();
        if (WORLDS.iter().any(|(world, _, _)| *world == heading)
            || matches!(heading, "Reception" | "Thanks"))
            && !positions.contains_key(heading)
        {
            positions.insert(heading.to_owned(), index);
        }
    }
    let mut result = HashMap::new();
    for (index, (world, _, _)) in WORLDS.iter().enumerate() {
        let start = *positions
            .get(*world)
            .ok_or_else(|| format!("walkthrough has no {world} section"))?;
        let next = if *world == "Multi Infinite" {
            "Reception"
        } else if let Some((next, _, _)) = WORLDS.get(index + 1) {
            next
        } else {
            "Thanks"
        };
        let end = *positions
            .get(next)
            .ok_or_else(|| format!("walkthrough has no {next} section"))?;
        result.insert((*world).to_owned(), lines[start + 1..end].join("\n"));
    }
    Ok(result)
}

fn numbered_items(text: &str) -> Result<HashMap<usize, String>, String> {
    let pattern = Regex::new(r"(?m)^\*\*([1-9][0-9]*)(?: ?[^*]*)?\*\*").map_err(display_error)?;
    let matches = pattern.captures_iter(text).collect::<Vec<_>>();
    let mut items = HashMap::new();
    for (index, captures) in matches.iter().enumerate() {
        let whole = captures.get(0).expect("whole match");
        let number = captures[1].parse::<usize>().map_err(display_error)?;
        let end = matches
            .get(index + 1)
            .and_then(|next| next.get(0))
            .map(|next| next.start())
            .unwrap_or(text.len());
        items.insert(number, text[whole.end()..end].to_owned());
    }
    Ok(items)
}

fn directions(text: &str, world: &str, number: usize) -> Result<String, String> {
    let text = if matches!((world, number), ("Empty", 14) | ("Clone", 5 | 6)) {
        text.split_once("Normal:")
            .map(|(_, value)| value)
            .ok_or("expected Normal variant")?
    } else {
        text
    };
    let annotations = Regex::new(r"\[[^]]*]|\([^)]*\)").map_err(display_error)?;
    let cleaned = annotations.replace_all(text, " ");
    let values = cleaned
        .split(|character: char| !character.is_ascii_uppercase())
        .filter(|value| {
            !value.is_empty()
                && value
                    .chars()
                    .all(|character| matches!(character, 'U' | 'D' | 'L' | 'R'))
        })
        .map(str::to_owned)
        .collect::<Vec<_>>();
    if values.is_empty() {
        Err(format!("no moves found for {world} {number}"))
    } else {
        Ok(values.join(" "))
    }
}

fn display_error(error: impl std::fmt::Display) -> String {
    error.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_numbered_direction_groups() {
        let items = numbered_items("**1 First**\nUUR D\n**2 Second**\nL R").unwrap();
        assert_eq!(directions(&items[&1], "Intro", 1).unwrap(), "UUR D");
        assert_eq!(directions(&items[&2], "Intro", 2).unwrap(), "L R");
    }
}
