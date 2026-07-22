//! Frozen puzzle-entry and post-puzzle overworld states.
//!
//! The entry states contain no walkthrough directions. The companion map
//! checkpoints preserve transition heights and reward state without exposing
//! any puzzle action sequence.

use std::collections::{BTreeMap, HashSet};
use std::fs::File;
use std::io::Read;
use std::path::Path;

use flate2::read::GzDecoder;
use tar::Archive;

use crate::{Campaign, GameState};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CampaignEntry {
    pub ordinal: usize,
    pub id: String,
    pub state: GameState,
    pub overworld: GameState,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CampaignEntries {
    pub levels: Vec<CampaignEntry>,
}

impl CampaignEntries {
    pub fn load(path: impl AsRef<Path>) -> Result<Self, String> {
        let path = path.as_ref();
        let entry_states = load_states(path, "campaign-entry")?;
        let overworld_path = path.with_file_name("overworld.tar.gz");
        let mut overworld_states = load_states(&overworld_path, "overworld-checkpoint")?;
        let mut levels = BTreeMap::new();
        for (ordinal, (id, state)) in entry_states {
            if state.overworld || state.push_target_level != id {
                return Err(format!(
                    "campaign entry {ordinal} targets {:?} instead of {id:?}",
                    state.push_target_level
                ));
            }
            let (overworld_id, overworld) = overworld_states.remove(&ordinal).ok_or_else(|| {
                format!("overworld checkpoint is missing ordinal {ordinal} {id:?}")
            })?;
            if !overworld.overworld || overworld_id != id {
                return Err(format!(
                    "overworld checkpoint {ordinal} {overworld_id:?} does not match {id:?}"
                ));
            }
            if levels
                .insert(
                    ordinal,
                    CampaignEntry {
                        ordinal,
                        id: id.to_owned(),
                        state,
                        overworld,
                    },
                )
                .is_some()
            {
                return Err(format!("duplicate campaign-entry ordinal {ordinal}"));
            }
        }
        if !overworld_states.is_empty() {
            return Err("overworld checkpoint archive has extra entries".to_owned());
        }
        let levels = levels.into_values().collect::<Vec<_>>();
        for (index, level) in levels.iter().enumerate() {
            if level.ordinal != index + 1 {
                return Err(format!(
                    "expected campaign-entry ordinal {}, found {}",
                    index + 1,
                    level.ordinal
                ));
            }
        }
        Ok(Self { levels })
    }

    pub fn validate_against(&self, campaign: &Campaign) -> Result<(), String> {
        if self.levels.len() != 86 {
            return Err(format!(
                "complete campaign requires 86 entry states, found {}",
                self.levels.len()
            ));
        }
        let expected = campaign.puzzle_ids().into_iter().collect::<HashSet<_>>();
        let actual = self
            .levels
            .iter()
            .map(|level| {
                level
                    .id
                    .split_once("__")
                    .map_or(level.id.as_str(), |(root, _)| root)
            })
            .collect::<HashSet<_>>();
        if actual != expected {
            let missing = expected.difference(&actual).copied().collect::<Vec<_>>();
            let extra = actual.difference(&expected).copied().collect::<Vec<_>>();
            return Err(format!(
                "campaign entry mismatch; missing={missing:?}, extra={extra:?}"
            ));
        }
        Ok(())
    }

    pub fn find(&self, reference: &str) -> Option<&CampaignEntry> {
        reference
            .parse::<usize>()
            .ok()
            .and_then(|ordinal| self.levels.get(ordinal.checked_sub(1)?))
            .or_else(|| self.levels.iter().find(|level| level.id == reference))
    }
}

fn load_states(path: &Path, label: &str) -> Result<BTreeMap<usize, (String, GameState)>, String> {
    let file =
        File::open(path).map_err(|error| format!("could not open {}: {error}", path.display()))?;
    let mut archive = Archive::new(GzDecoder::new(file));
    let mut states = BTreeMap::new();
    for item in archive
        .entries()
        .map_err(|error| format!("invalid {label} archive: {error}"))?
    {
        let mut item = item.map_err(|error| format!("invalid {label} archive item: {error}"))?;
        if !item.header().entry_type().is_file() {
            continue;
        }
        let item_path = item
            .path()
            .map_err(|error| format!("invalid {label} path: {error}"))?;
        let Some(file_name) = item_path
            .file_name()
            .and_then(|name| name.to_str())
            .map(str::to_owned)
        else {
            return Err(format!("{label} filename is not UTF-8"));
        };
        if file_name.starts_with("._") {
            continue;
        }
        let stem = file_name
            .strip_suffix(".state")
            .ok_or_else(|| format!("unexpected {label} file {file_name:?}"))?;
        let (ordinal, id) = stem
            .split_once('-')
            .ok_or_else(|| format!("{label} filename has no ordinal: {file_name:?}"))?;
        let ordinal = ordinal
            .parse::<usize>()
            .map_err(|error| format!("invalid {label} ordinal in {file_name:?}: {error}"))?;
        let mut source = String::new();
        item.read_to_string(&mut source)
            .map_err(|error| format!("{file_name} is not UTF-8: {error}"))?;
        let state = GameState::parse(&source)
            .map_err(|error| format!("invalid {label} {file_name}: {error}"))?;
        if states.insert(ordinal, (id.to_owned(), state)).is_some() {
            return Err(format!("duplicate {label} ordinal {ordinal}"));
        }
    }
    Ok(states)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn root() -> &'static Path {
        crate::data_root()
    }

    #[test]
    fn loads_all_solution_free_entry_states() {
        let campaign = Campaign::load_gzip(root().join("campaign").join("merged_binary.gz"))
            .expect("campaign should load");
        let entries = CampaignEntries::load(root().join("campaign").join("entries.tar.gz"))
            .expect("entries should load");
        entries
            .validate_against(&campaign)
            .expect("entries should cover the campaign");
        assert_eq!(entries.levels.first().expect("first").id, "level47");
        assert_eq!(
            entries.levels.last().expect("last").id,
            "modular8a__island1"
        );
    }
}
