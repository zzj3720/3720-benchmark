//! Frozen, solution-free puzzle entry states.
//!
//! These states were captured from the locally owned original build while
//! entering each puzzle. They contain no walkthrough directions or
//! checkpoints and are safe to package with the rules engine.

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
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CampaignEntries {
    pub levels: Vec<CampaignEntry>,
}

impl CampaignEntries {
    pub fn load(path: impl AsRef<Path>) -> Result<Self, String> {
        let path = path.as_ref();
        let file = File::open(path)
            .map_err(|error| format!("could not open {}: {error}", path.display()))?;
        let mut archive = Archive::new(GzDecoder::new(file));
        let mut levels = BTreeMap::new();
        for item in archive
            .entries()
            .map_err(|error| format!("invalid campaign-entry archive: {error}"))?
        {
            let mut item =
                item.map_err(|error| format!("invalid campaign-entry archive item: {error}"))?;
            if !item.header().entry_type().is_file() {
                continue;
            }
            let path = item
                .path()
                .map_err(|error| format!("invalid campaign-entry path: {error}"))?;
            let Some(file_name) = path
                .file_name()
                .and_then(|name| name.to_str())
                .map(str::to_owned)
            else {
                return Err("campaign-entry filename is not UTF-8".to_owned());
            };
            if file_name.starts_with("._") {
                continue;
            }
            let Some(stem) = file_name.strip_suffix(".state") else {
                return Err(format!("unexpected campaign-entry file {file_name:?}"));
            };
            let (ordinal, id) = stem
                .split_once('-')
                .ok_or_else(|| format!("campaign-entry filename has no ordinal: {file_name:?}"))?;
            let ordinal = ordinal.parse::<usize>().map_err(|error| {
                format!("invalid campaign-entry ordinal in {file_name:?}: {error}")
            })?;
            let mut source = String::new();
            item.read_to_string(&mut source)
                .map_err(|error| format!("{file_name} is not UTF-8: {error}"))?;
            let state = GameState::parse(&source)
                .map_err(|error| format!("invalid campaign entry {file_name}: {error}"))?;
            if state.overworld || state.push_target_level != id {
                return Err(format!(
                    "{file_name} targets {:?} instead of {id:?}",
                    state.push_target_level
                ));
            }
            if levels
                .insert(
                    ordinal,
                    CampaignEntry {
                        ordinal,
                        id: id.to_owned(),
                        state,
                    },
                )
                .is_some()
            {
                return Err(format!("duplicate campaign-entry ordinal {ordinal}"));
            }
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
