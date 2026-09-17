use std::collections::BTreeSet;

use serde::Serialize;
use serde_json::Value;

use crate::campaign::Campaign;
use crate::engine::{Board, BoardSnapshot, Direction, StepResult};

#[derive(Debug)]
pub struct Session<'a> {
    campaign: &'a Campaign,
    active: Option<Active>,
    solved: BTreeSet<String>,
}

#[derive(Debug)]
struct Active {
    tier_index: usize,
    level_index: usize,
    board: Board,
}

#[derive(Debug, Serialize)]
pub struct Snapshot<'a> {
    pub schema: &'static str,
    pub campaign: CampaignSnapshot<'a>,
    pub tiers: Vec<TierSnapshot<'a>>,
    pub level: Option<LevelSnapshot<'a>>,
    pub board: Option<BoardSnapshot>,
    pub symbols: Symbols,
}

#[derive(Debug, Serialize)]
pub struct CampaignSnapshot<'a> {
    pub id: &'a str,
    pub title: &'a str,
    pub score: usize,
    pub max_score: usize,
    pub complete: bool,
}

#[derive(Debug, Serialize)]
pub struct TierSnapshot<'a> {
    pub id: &'a str,
    pub title: &'a str,
    pub difficulty: &'a str,
    pub status: &'static str,
    pub solved: usize,
    pub total: usize,
    pub unlock_after: usize,
    pub remaining_to_unlock_next: usize,
}

#[derive(Debug, Serialize)]
pub struct LevelSnapshot<'a> {
    pub id: &'a str,
    pub title: &'a str,
    pub source_id: &'a str,
    pub tier_id: &'a str,
    pub tier_title: &'a str,
    pub number: usize,
    pub solved: bool,
}

#[derive(Debug, Serialize)]
pub struct LevelSummary<'a> {
    pub id: &'a str,
    pub title: &'a str,
    pub tier_id: &'a str,
    pub number: usize,
    pub unlocked: bool,
    pub solved: bool,
    pub width: usize,
    pub height: usize,
}

#[derive(Debug, Serialize)]
pub struct Symbols {
    pub wall: char,
    pub floor: char,
    pub goal: char,
    pub box_tile: char,
    pub box_on_goal: char,
    pub player: char,
    pub player_on_goal: char,
}

impl<'a> Session<'a> {
    pub fn new(campaign: &'a Campaign) -> Self {
        Self {
            campaign,
            active: None,
            solved: BTreeSet::new(),
        }
    }

    pub fn select(&mut self, level_id: &str) -> Result<(), String> {
        let (tier_index, level_index, level) = self
            .campaign
            .find_level(level_id)
            .ok_or_else(|| format!("unknown level: {level_id}"))?;
        if !self.tier_unlocked(tier_index) {
            return Err(format!(
                "tier {} is locked; solve at least half of the preceding tier first",
                self.campaign.tiers[tier_index].id
            ));
        }
        self.active = Some(Active {
            tier_index,
            level_index,
            board: Board::from_level(level),
        });
        Ok(())
    }

    pub fn move_many(
        &mut self,
        directions: &[Direction],
    ) -> Result<(Vec<StepResult>, bool), String> {
        let (results, newly_solved, _) = self.move_many_inner(directions, false)?;
        Ok((results, newly_solved))
    }

    pub fn move_many_observed(
        &mut self,
        directions: &[Direction],
    ) -> Result<(Vec<StepResult>, bool, Vec<Value>), String> {
        self.move_many_inner(directions, true)
    }

    fn move_many_inner(
        &mut self,
        directions: &[Direction],
        observe: bool,
    ) -> Result<(Vec<StepResult>, bool, Vec<Value>), String> {
        if directions.is_empty() {
            return Err("provide at least one direction".to_owned());
        }
        if directions.len() > 64 {
            return Err("a move command accepts at most 64 directions".to_owned());
        }
        let active = self
            .active
            .as_ref()
            .ok_or_else(|| "select a level before moving".to_owned())?;
        let level_id = self.campaign.tiers[active.tier_index].levels[active.level_index]
            .id
            .clone();
        let mut results = Vec::with_capacity(directions.len());
        let mut snapshots = Vec::with_capacity(if observe { directions.len() } else { 0 });
        let mut newly_solved = false;
        for direction in directions {
            let result = self
                .active
                .as_mut()
                .expect("active level checked above")
                .board
                .step(*direction);
            let solved = result.solved;
            results.push(result);
            if solved {
                newly_solved = self.solved.insert(level_id.clone());
            }
            if observe {
                snapshots.push(
                    serde_json::to_value(self.snapshot())
                        .map_err(|error| format!("could not serialize observed move: {error}"))?,
                );
            }
            if solved {
                break;
            }
        }
        Ok((results, newly_solved, snapshots))
    }

    pub fn undo(&mut self, steps: usize) -> Result<usize, String> {
        Ok(self.undo_inner(steps, false)?.0)
    }

    pub fn undo_observed(&mut self, steps: usize) -> Result<(usize, Vec<Value>), String> {
        self.undo_inner(steps, true)
    }

    fn undo_inner(&mut self, steps: usize, observe: bool) -> Result<(usize, Vec<Value>), String> {
        if steps == 0 || steps > 64 {
            return Err("undo steps must be between 1 and 64".to_owned());
        }
        if self.active.is_none() {
            return Err("select a level before undoing".to_owned());
        }
        let mut undone = 0;
        let mut snapshots = Vec::with_capacity(if observe { steps } else { 0 });
        for _ in 0..steps {
            let applied = self
                .active
                .as_mut()
                .expect("active level checked above")
                .board
                .undo(1);
            if applied == 0 {
                break;
            }
            undone += 1;
            if observe {
                snapshots.push(
                    serde_json::to_value(self.snapshot())
                        .map_err(|error| format!("could not serialize observed undo: {error}"))?,
                );
            }
        }
        Ok((undone, snapshots))
    }

    pub fn reset(&mut self) -> Result<(), String> {
        let active = self
            .active
            .as_mut()
            .ok_or_else(|| "select a level before resetting".to_owned())?;
        active.board.reset();
        Ok(())
    }

    pub fn snapshot(&self) -> Snapshot<'_> {
        let tiers = self
            .campaign
            .tiers
            .iter()
            .enumerate()
            .map(|(index, tier)| {
                let solved = self.solved_in_tier(index);
                let unlocked = self.tier_unlocked(index);
                TierSnapshot {
                    id: &tier.id,
                    title: &tier.title,
                    difficulty: &tier.difficulty,
                    status: if !unlocked {
                        "locked"
                    } else if solved == tier.levels.len() {
                        "complete"
                    } else {
                        "unlocked"
                    },
                    solved,
                    total: tier.levels.len(),
                    unlock_after: tier.unlock_after,
                    remaining_to_unlock_next: if index + 1 < self.campaign.tiers.len() {
                        tier.unlock_after.saturating_sub(solved)
                    } else {
                        0
                    },
                }
            })
            .collect();
        let (level, board) = self.active.as_ref().map_or((None, None), |active| {
            let tier = &self.campaign.tiers[active.tier_index];
            let level = &tier.levels[active.level_index];
            (
                Some(LevelSnapshot {
                    id: &level.id,
                    title: &level.title,
                    source_id: &level.source_id,
                    tier_id: &tier.id,
                    tier_title: &tier.title,
                    number: active.level_index + 1,
                    solved: self.solved.contains(&level.id),
                }),
                Some(active.board.snapshot()),
            )
        });
        Snapshot {
            schema: "sokoban-state-v1",
            campaign: CampaignSnapshot {
                id: &self.campaign.id,
                title: &self.campaign.title,
                score: self.solved.len(),
                max_score: self.campaign.max_score(),
                complete: self.solved.len() == self.campaign.max_score(),
            },
            tiers,
            level,
            board,
            symbols: Symbols {
                wall: '#',
                floor: ' ',
                goal: '.',
                box_tile: '$',
                box_on_goal: '*',
                player: '@',
                player_on_goal: '+',
            },
        }
    }

    pub fn levels(&self, tier_filter: Option<&str>) -> Result<Vec<LevelSummary<'_>>, String> {
        if tier_filter.is_some_and(|id| !self.campaign.tiers.iter().any(|tier| tier.id == id)) {
            return Err(format!("unknown tier: {}", tier_filter.unwrap_or_default()));
        }
        Ok(self
            .campaign
            .tiers
            .iter()
            .enumerate()
            .filter(|(_, tier)| tier_filter.is_none_or(|id| tier.id == id))
            .flat_map(|(tier_index, tier)| {
                tier.levels
                    .iter()
                    .enumerate()
                    .map(move |(level_index, level)| LevelSummary {
                        id: level.id.as_str(),
                        title: level.title.as_str(),
                        tier_id: tier.id.as_str(),
                        number: level_index + 1,
                        unlocked: self.tier_unlocked(tier_index),
                        solved: self.solved.contains(&level.id),
                        width: level.width,
                        height: level.height,
                    })
            })
            .collect())
    }

    pub fn score(&self) -> usize {
        self.solved.len()
    }

    pub fn campaign(&self) -> &Campaign {
        self.campaign
    }

    fn tier_unlocked(&self, tier_index: usize) -> bool {
        (0..tier_index)
            .all(|index| self.solved_in_tier(index) >= self.campaign.tiers[index].unlock_after)
    }

    fn solved_in_tier(&self, tier_index: usize) -> usize {
        self.campaign.tiers[tier_index]
            .levels
            .iter()
            .filter(|level| self.solved.contains(&level.id))
            .count()
    }
}

#[cfg(test)]
mod tests {
    use crate::{Level, Tier};

    use super::*;

    fn tiny_level(id: &str) -> Level {
        Level {
            id: id.to_owned(),
            source_id: id.to_owned(),
            title: id.to_owned(),
            width: 5,
            height: 3,
            rows: vec!["#####".into(), "#@$.#".into(), "#####".into()],
        }
    }

    #[test]
    fn solving_half_unlocks_the_next_tier() {
        let campaign = Campaign {
            schema: "sokoban-campaign-v1".into(),
            id: "test".into(),
            title: "Test".into(),
            tiers: vec![
                Tier {
                    id: "easy".into(),
                    title: "Easy".into(),
                    difficulty: "Easy".into(),
                    source_url: "test".into(),
                    unlock_after: 1,
                    levels: vec![tiny_level("easy-1"), tiny_level("easy-2")],
                },
                Tier {
                    id: "hard".into(),
                    title: "Hard".into(),
                    difficulty: "Hard".into(),
                    source_url: "test".into(),
                    unlock_after: 1,
                    levels: vec![tiny_level("hard-1")],
                },
            ],
        };
        let mut session = Session::new(&campaign);
        assert!(session.select("hard-1").is_err());
        session.select("easy-1").expect("select easy");
        let (_, newly_solved) = session.move_many(&[Direction::Right]).expect("solve easy");
        assert!(newly_solved);
        assert_eq!(session.snapshot().tiers[1].status, "unlocked");
        session
            .select("hard-1")
            .expect("select newly unlocked tier");
        assert_eq!(
            session
                .snapshot()
                .tiers
                .last()
                .expect("last tier")
                .remaining_to_unlock_next,
            0
        );
    }
}
