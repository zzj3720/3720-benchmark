use std::fs;
use std::path::Path;

use crate::campaign::{CAMPAIGN_ID, LEVELS, STATE_HEADER, level_index};
use crate::engine::Direction;

#[derive(Clone)]
pub struct State {
    pub histories: Vec<Vec<Direction>>,
    pub solved_order: Vec<usize>,
    pub selected: Option<usize>,
}

impl State {
    pub fn new() -> Self {
        Self {
            histories: vec![Vec::new(); LEVELS.len()],
            solved_order: Vec::new(),
            selected: level_index("a1"),
        }
    }

    pub fn load(path: &Path) -> Result<Self, String> {
        if !path.exists() {
            return Ok(Self::new());
        }
        let text = fs::read_to_string(path)
            .map_err(|error| format!("failed to read {}: {error}", path.display()))?;
        let mut lines = text.lines();
        if lines.next() != Some(STATE_HEADER) {
            return Err("unsupported state format".to_string());
        }
        if lines.next() != Some(&format!("campaign {CAMPAIGN_ID}")) {
            return Err("state belongs to a different campaign".to_string());
        }

        let mut state = Self::new();
        let mut current = None;
        let mut seen_levels = vec![false; LEVELS.len()];
        for line in lines {
            if let Some(reference) = line.strip_prefix("selected ") {
                state.selected = match reference {
                    "-" => None,
                    reference => Some(
                        level_index(reference)
                            .ok_or_else(|| format!("unknown selected level: {reference}"))?,
                    ),
                };
                continue;
            }
            if let Some(reference) = line.strip_prefix("solved ") {
                let index = level_index(reference)
                    .ok_or_else(|| format!("unknown solved level: {reference}"))?;
                if state.solved_order.contains(&index) {
                    return Err(format!("duplicate solved level: {reference}"));
                }
                state.solved_order.push(index);
                continue;
            }
            if let Some(reference) = line.strip_prefix("level ") {
                let index = level_index(reference)
                    .ok_or_else(|| format!("unknown level in state: {reference}"))?;
                if current.is_some_and(|previous| index <= previous) {
                    return Err("state levels are out of order or duplicated".to_string());
                }
                current = Some(index);
                seen_levels[index] = true;
                continue;
            }

            let index = current.ok_or_else(|| "state data appears before a level".to_string())?;
            state.histories[index].push(Direction::parse(line)?);
        }

        if seen_levels.iter().any(|&seen| !seen) {
            return Err("state does not contain every campaign level".to_string());
        }
        Ok(state)
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        let selected = self
            .selected
            .map(|index| LEVELS[index].reference)
            .unwrap_or("-");
        let mut text = format!("{STATE_HEADER}\ncampaign {CAMPAIGN_ID}\nselected {selected}\n");
        for &index in &self.solved_order {
            text.push_str(&format!("solved {}\n", LEVELS[index].reference));
        }
        for (entry, actions) in LEVELS.iter().zip(&self.histories) {
            text.push_str(&format!("level {}\n", entry.reference));
            for action in actions {
                text.push_str(action.as_str());
                text.push('\n');
            }
        }
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .map_err(|error| format!("failed to create {}: {error}", parent.display()))?;
        }
        let temporary = path.with_extension("tmp");
        fs::write(&temporary, text)
            .map_err(|error| format!("failed to write {}: {error}", temporary.display()))?;
        fs::rename(&temporary, path)
            .map_err(|error| format!("failed to replace {}: {error}", path.display()))
    }
}

impl Default for State {
    fn default() -> Self {
        Self::new()
    }
}
