use std::collections::{HashMap, HashSet};
use std::path::Path;

use parabox_terminal::campaign::{LEVELS, area};
use parabox_terminal::engine::{Direction, load_level};
use serde_json::{Value, json};

const SAMPLE_COUNT: usize = 64;

struct Candidate {
    quality: usize,
    rectangular: bool,
    value: Value,
}

fn direction(ch: char) -> Result<Direction, String> {
    match ch {
        'U' => Ok(Direction::Up),
        'D' => Ok(Direction::Down),
        'L' => Ok(Direction::Left),
        'R' => Ok(Direction::Right),
        _ => Err(format!("invalid oracle direction: {ch}")),
    }
}

fn main() -> Result<(), String> {
    let campaign_dir = std::env::args()
        .nth(1)
        .ok_or_else(|| "usage: render-qa CAMPAIGN_DIR".to_string())?;
    let histories: HashMap<_, _> = include_str!("../../data/oracle/oracle.tsv")
        .lines()
        .filter(|line| !line.starts_with('#') && !line.trim().is_empty())
        .map(|line| {
            let (reference, history) = line
                .split_once('\t')
                .expect("oracle row has a tab separator");
            (reference, history)
        })
        .collect();
    let mut candidates = Vec::new();

    for entry in LEVELS.iter() {
        let Some(history) = histories.get(entry.reference) else {
            continue;
        };
        let moves: Vec<_> = history.chars().filter(|ch| "UDLR".contains(*ch)).collect();
        let mut game = load_level(Path::new(&campaign_dir).join("levels").join(entry.file))?;
        let mut best = None;

        for (index, ch) in moves.iter().copied().enumerate() {
            game.move_player(direction(ch)?);
            let space = game.current_space()?;
            let depth = space.path.len().saturating_sub(1);
            if depth == 0 {
                continue;
            }
            let scene = game.observer_scene()?;
            let focus = scene
                .spaces
                .iter()
                .find(|candidate| candidate.id == scene.focus_space)
                .ok_or_else(|| "observer scene is missing its focus space".to_string())?;
            let rectangular_spaces = scene
                .spaces
                .iter()
                .filter(|space| space.width != space.height)
                .count();
            let rectangular = rectangular_spaces > 0;
            let nested_boxes = focus
                .blocks
                .iter()
                .filter(|block| block.kind == "box" && block.subspace.is_some())
                .count();
            let boundary_openings = focus
                .map
                .iter()
                .enumerate()
                .map(|(row, line)| {
                    line.chars()
                        .enumerate()
                        .filter(|(column, symbol)| {
                            (row == 0
                                || row + 1 == focus.map.len()
                                || *column == 0
                                || *column + 1 == line.chars().count())
                                && !matches!(*symbol, '#' | '!')
                        })
                        .count()
                })
                .sum::<usize>();
            let middle = (index + 1).min(moves.len().saturating_sub(index + 1));
            let quality = depth * 10_000
                + usize::from(scene.camera_flip_h) * 2_000
                + usize::from(rectangular) * 1_000
                + nested_boxes * 100
                + boundary_openings * 10
                + middle;
            let value = json!({
                "reference": entry.reference,
                "title": entry.title,
                "area": area(entry.area).name,
                "step": index + 1,
                "total_steps": moves.len(),
                "path": space.path.join(" › "),
                "features": {
                    "depth": depth,
                    "rectangular": rectangular,
                    "rectangular_spaces": rectangular_spaces,
                    "flipped": scene.camera_flip_h,
                    "visible_spaces": scene.spaces.len(),
                    "nested_boxes": nested_boxes,
                    "boundary_openings": boundary_openings,
                },
                "state": {
                    "level": {
                        "reference": entry.reference,
                        "title": entry.title,
                        "area": area(entry.area).name,
                    },
                    "space": space,
                    "observer_scene": scene,
                },
            });
            if best
                .as_ref()
                .is_none_or(|candidate: &Candidate| quality > candidate.quality)
            {
                best = Some(Candidate {
                    quality,
                    rectangular,
                    value,
                });
            }
        }
        if let Some(candidate) = best {
            candidates.push(candidate);
        }
    }

    if candidates.len() < SAMPLE_COUNT {
        return Err(format!(
            "only {} levels produced recursive states",
            candidates.len()
        ));
    }
    let mut selected: HashSet<_> = (0..SAMPLE_COUNT)
        .map(|slot| {
            let start = slot * candidates.len() / SAMPLE_COUNT;
            let end = (slot + 1) * candidates.len() / SAMPLE_COUNT;
            start
                + candidates[start..end]
                    .iter()
                    .enumerate()
                    .max_by_key(|(_, candidate)| candidate.quality)
                    .expect("sample bucket is not empty")
                    .0
        })
        .collect();
    for required in candidates
        .iter()
        .enumerate()
        .filter_map(|(index, candidate)| candidate.rectangular.then_some(index))
    {
        if selected.contains(&required) {
            continue;
        }
        let replaced = selected
            .iter()
            .copied()
            .filter(|index| !candidates[*index].rectangular)
            .min_by_key(|index| index.abs_diff(required))
            .expect("sample contains a replaceable square state");
        selected.remove(&replaced);
        selected.insert(required);
    }
    let mut selected: Vec<_> = selected.into_iter().collect();
    selected.sort_unstable();
    let samples: Vec<_> = selected
        .into_iter()
        .map(|index| candidates[index].value.clone())
        .collect();
    println!(
        "{}",
        serde_json::to_string_pretty(&json!({
            "schema": "parabox-render-qa-v1",
            "sample_count": samples.len(),
            "eligible_levels": candidates.len(),
            "samples": samples,
        }))
        .map_err(|error| format!("failed to serialize QA samples: {error}"))?
    );
    Ok(())
}
