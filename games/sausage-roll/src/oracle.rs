//! Development-only import of the complete public walkthrough.
//!
//! This module deliberately does not participate in scoring. It turns the
//! walkthrough into typed, per-puzzle checkpoints so the Rust mechanics can
//! be developed against stable original-game states. The archive is excluded
//! from the future Agent image.

use std::collections::{BTreeMap, HashSet};
use std::fs::File;
use std::io::Read;
use std::path::Path;

use flate2::read::GzDecoder;
use tar::Archive;

use crate::{Campaign, Coord, Direction, Game3d, GameState, Replay};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OracleEntity {
    pub id: i32,
    pub pos: Coord,
    pub direction: Direction,
    pub rotation: i32,
    pub cook_data: i32,
}

impl OracleEntity {
    fn parse(source: &str) -> Result<Self, String> {
        let (id, fields) = source
            .split_once(':')
            .ok_or_else(|| format!("checkpoint entity has no id separator: {source:?}"))?;
        let values = fields.split(',').collect::<Vec<_>>();
        if values.len() != 6 {
            return Err(format!(
                "checkpoint entity requires 6 fields, got {} in {source:?}",
                values.len()
            ));
        }
        let integer = |index: usize, name: &str| {
            values[index]
                .parse::<i32>()
                .map_err(|error| format!("invalid checkpoint {name}: {error}"))
        };
        Ok(Self {
            id: id
                .parse()
                .map_err(|error| format!("invalid checkpoint entity id: {error}"))?,
            pos: Coord::new(integer(0, "x")?, integer(1, "y")?, integer(2, "z")?),
            direction: Direction::from_i32(integer(3, "direction")?)?,
            rotation: integer(4, "rotation")?,
            cook_data: integer(5, "cook data")?,
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OracleCheckpoint {
    pub entities: Vec<OracleEntity>,
}

impl OracleCheckpoint {
    fn parse(source: &str) -> Result<Self, String> {
        let entities = source
            .trim()
            .split(';')
            .filter(|item| !item.is_empty())
            .map(OracleEntity::parse)
            .collect::<Result<Vec<_>, _>>()?;
        if entities.is_empty() {
            return Err("oracle checkpoint has no entities".to_owned());
        }
        if entities
            .iter()
            .map(|entity| entity.id)
            .collect::<HashSet<_>>()
            .len()
            != entities.len()
        {
            return Err("oracle checkpoint contains duplicate entity ids".to_owned());
        }
        Ok(Self { entities })
    }

    pub fn player(&self) -> Option<&OracleEntity> {
        self.entities.iter().find(|entity| entity.id == 67)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OracleSegment {
    pub ordinal: usize,
    pub id: String,
    pub entry: GameState,
    pub replay: Replay,
    /// Stable original-engine states after each non-final puzzle action.
    ///
    /// The final action returns to the overworld, so its puzzle state is
    /// represented by `GuidedReplay::complete()` instead of a checkpoint.
    pub checkpoints: Vec<OracleCheckpoint>,
}

#[derive(Default)]
struct SegmentSources {
    state: Option<String>,
    replay: Option<String>,
    trace: Option<String>,
}

impl OracleSegment {
    fn parse(stem: &str, sources: SegmentSources) -> Result<Self, String> {
        let (ordinal, id) = stem
            .split_once('-')
            .ok_or_else(|| format!("oracle segment stem has no ordinal: {stem:?}"))?;
        let ordinal = ordinal
            .parse::<usize>()
            .map_err(|error| format!("invalid oracle ordinal in {stem:?}: {error}"))?;
        let entry = GameState::parse(
            sources
                .state
                .as_deref()
                .ok_or_else(|| format!("missing entry state for {stem}"))?,
        )
        .map_err(|error| format!("invalid entry state for {stem}: {error}"))?;
        if entry.overworld || entry.push_target_level != id {
            return Err(format!(
                "{stem} entry targets {:?} instead of {id:?}",
                entry.push_target_level
            ));
        }
        let replay = Replay::parse(
            sources
                .replay
                .as_deref()
                .ok_or_else(|| format!("missing direction replay for {stem}"))?,
        )
        .map_err(|error| format!("invalid direction replay for {stem}: {error}"))?;
        let checkpoints = sources
            .trace
            .as_deref()
            .ok_or_else(|| format!("missing checkpoint trace for {stem}"))?
            .lines()
            .map(OracleCheckpoint::parse)
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| format!("invalid checkpoint trace for {stem}: {error}"))?;
        if checkpoints.len() + 1 != replay.directions.len() {
            return Err(format!(
                "{stem} has {} actions but {} non-final checkpoints",
                replay.directions.len(),
                checkpoints.len()
            ));
        }
        Ok(Self {
            ordinal,
            id: id.to_owned(),
            entry,
            replay,
            checkpoints,
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OracleCampaign {
    pub segments: Vec<OracleSegment>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EngineVerification {
    pub puzzles: usize,
    pub actions: usize,
    pub checkpoints: usize,
}

impl OracleCampaign {
    pub fn load(path: impl AsRef<Path>) -> Result<Self, String> {
        let path = path.as_ref();
        let file = File::open(path)
            .map_err(|error| format!("could not open {}: {error}", path.display()))?;
        let decoder = GzDecoder::new(file);
        let mut archive = Archive::new(decoder);
        let mut sources = BTreeMap::<String, SegmentSources>::new();
        for entry in archive
            .entries()
            .map_err(|error| format!("invalid oracle archive: {error}"))?
        {
            let mut entry =
                entry.map_err(|error| format!("invalid oracle archive entry: {error}"))?;
            if !entry.header().entry_type().is_file() {
                continue;
            }
            let path = entry
                .path()
                .map_err(|error| format!("invalid oracle archive path: {error}"))?;
            let Some(file_name) = path
                .file_name()
                .and_then(|name| name.to_str())
                .map(str::to_owned)
            else {
                continue;
            };
            if file_name.starts_with("._") {
                continue;
            }
            let Some((stem, kind)) = file_name.rsplit_once('.') else {
                continue;
            };
            if !matches!(kind, "state" | "dem" | "trace") {
                continue;
            }
            let mut text = String::new();
            entry
                .read_to_string(&mut text)
                .map_err(|error| format!("{file_name} is not UTF-8: {error}"))?;
            let segment = sources.entry(stem.to_owned()).or_default();
            let slot = match kind {
                "state" => &mut segment.state,
                "dem" => &mut segment.replay,
                "trace" => &mut segment.trace,
                _ => unreachable!("filtered above"),
            };
            if slot.replace(text).is_some() {
                return Err(format!("duplicate {kind} entry for {stem}"));
            }
        }
        let mut segments = sources
            .into_iter()
            .map(|(stem, sources)| OracleSegment::parse(&stem, sources))
            .collect::<Result<Vec<_>, _>>()?;
        segments.sort_by_key(|segment| segment.ordinal);
        for (index, segment) in segments.iter().enumerate() {
            if segment.ordinal != index + 1 {
                return Err(format!(
                    "expected oracle ordinal {}, found {}",
                    index + 1,
                    segment.ordinal
                ));
            }
        }
        Ok(Self { segments })
    }

    pub fn action_count(&self) -> usize {
        self.segments
            .iter()
            .map(|segment| segment.replay.directions.len())
            .sum()
    }

    pub fn validate_against(&self, campaign: &Campaign) -> Result<(), String> {
        if self.segments.len() != 86 {
            return Err(format!(
                "complete walkthrough requires 86 segments, found {}",
                self.segments.len()
            ));
        }
        let expected = campaign.puzzle_ids().into_iter().collect::<HashSet<_>>();
        let actual = self
            .segments
            .iter()
            .map(|segment| {
                segment
                    .id
                    .split_once("__")
                    .map_or(segment.id.as_str(), |(root, _)| root)
            })
            .collect::<HashSet<_>>();
        if actual != expected {
            let missing = expected.difference(&actual).copied().collect::<Vec<_>>();
            let extra = actual.difference(&expected).copied().collect::<Vec<_>>();
            return Err(format!(
                "walkthrough/campaign puzzle mismatch; missing={missing:?}, extra={extra:?}"
            ));
        }
        if self.action_count() != 11_769 {
            return Err(format!(
                "expected 11769 puzzle actions, found {}",
                self.action_count()
            ));
        }
        for segment in &self.segments {
            let mut replay = GuidedReplay::new(segment);
            for direction in &segment.replay.directions {
                replay.step(*direction)?;
            }
            if !replay.complete() {
                return Err(format!(
                    "{} did not consume its complete replay",
                    segment.id
                ));
            }
        }
        Ok(())
    }

    pub fn verify_with_engine(&self, campaign: &Campaign) -> Result<EngineVerification, String> {
        self.validate_against(campaign)?;
        let mut actions = 0;
        let mut checkpoints = 0;
        for segment in &self.segments {
            let mut game = Game3d::from_state(campaign, &segment.entry)
                .map_err(|error| format!("{} could not start: {error}", segment.id))?;
            for (index, direction) in segment.replay.directions.iter().copied().enumerate() {
                let accepted = game
                    .step(direction)
                    .map_err(|error| format!("{} action {}: {error}", segment.id, index + 1))?;
                if !accepted {
                    return Err(format!(
                        "{} action {} ({direction:?}) was rejected",
                        segment.id,
                        index + 1
                    ));
                }
                actions += 1;
                if let Some(checkpoint) = segment.checkpoints.get(index) {
                    verify_checkpoint(&game, checkpoint, &segment.id, index + 1)?;
                    checkpoints += 1;
                }
            }
            if !game.can_exit() {
                return Err(format!(
                    "{} finished the walkthrough but did not reach its exit",
                    segment.id
                ));
            }
        }
        Ok(EngineVerification {
            puzzles: self.segments.len(),
            actions,
            checkpoints,
        })
    }
}

fn verify_checkpoint(
    game: &Game3d<'_>,
    checkpoint: &OracleCheckpoint,
    puzzle: &str,
    action: usize,
) -> Result<(), String> {
    for expected in &checkpoint.entities {
        let actual = game
            .world
            .entity(expected.id)
            .ok_or_else(|| format!("{puzzle} action {action}: missing entity {}", expected.id))?;
        let actual_state = (
            actual.pos,
            actual.direction,
            actual.rotation,
            actual.cook_data,
        );
        let expected_state = (
            expected.pos,
            expected.direction,
            expected.rotation,
            expected.cook_data,
        );
        if actual_state != expected_state {
            return Err(format!(
                "{puzzle} action {action}: entity {} actual={actual_state:?} expected={expected_state:?}",
                expected.id
            ));
        }
    }
    Ok(())
}

#[derive(Clone, Debug)]
pub struct GuidedReplay<'a> {
    segment: &'a OracleSegment,
    action_index: usize,
}

impl<'a> GuidedReplay<'a> {
    pub fn new(segment: &'a OracleSegment) -> Self {
        Self {
            segment,
            action_index: 0,
        }
    }

    pub fn step(&mut self, direction: Direction) -> Result<Option<&'a OracleCheckpoint>, String> {
        let expected = self
            .segment
            .replay
            .directions
            .get(self.action_index)
            .copied()
            .ok_or_else(|| format!("{} replay is already complete", self.segment.id))?;
        if direction != expected {
            return Err(format!(
                "{} action {} is {direction:?}, expected {expected:?}",
                self.segment.id,
                self.action_index + 1
            ));
        }
        self.action_index += 1;
        if self.complete() {
            Ok(None)
        } else {
            Ok(self.segment.checkpoints.get(self.action_index - 1))
        }
    }

    pub fn complete(&self) -> bool {
        self.action_index == self.segment.replay.directions.len()
    }

    pub fn action_index(&self) -> usize {
        self.action_index
    }
}
