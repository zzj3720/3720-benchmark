use std::collections::BTreeSet;

use serde::Serialize;

use crate::campaign::{Campaign, OpeningDistributionProfile, ProofProfile};
use crate::engine::{ActionResult, Board, BoardSnapshot, Cell, GameStatus};

#[derive(Debug)]
pub struct Session<'a> {
    campaign: &'a Campaign,
    active: Option<Active>,
    attempted: BTreeSet<String>,
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
    pub guarantee: Guarantee,
    pub campaign: CampaignSnapshot<'a>,
    pub tiers: Vec<TierSnapshot<'a>>,
    pub level: Option<LevelSnapshot<'a>>,
    pub board: Option<BoardSnapshot>,
    pub symbols: Symbols,
}

#[derive(Debug, Serialize)]
pub struct Guarantee {
    pub first_click_safe: bool,
    pub safe_radius: usize,
    pub no_guess: bool,
    pub proof_rules: &'static str,
    pub difficulty_metric: &'static str,
    pub distribution_metric: &'static str,
}

#[derive(Debug, Serialize)]
pub struct CampaignSnapshot<'a> {
    pub id: &'a str,
    pub title: &'a str,
    pub score: usize,
    pub max_score: usize,
    pub complete: bool,
    pub status: &'static str,
}

#[derive(Debug, Serialize)]
pub struct TierSnapshot<'a> {
    pub id: &'a str,
    pub title: &'a str,
    pub difficulty: &'a str,
    pub status: &'static str,
    pub solved: usize,
    pub failed: usize,
    pub attempted: usize,
    pub total: usize,
    pub wins_required: usize,
    pub wins_remaining: usize,
    pub attempts_remaining: usize,
    pub opening_distribution: &'a OpeningDistributionProfile,
}

#[derive(Debug, Serialize)]
pub struct LevelSnapshot<'a> {
    pub id: &'a str,
    pub title: &'a str,
    pub tier_id: &'a str,
    pub tier_title: &'a str,
    pub number: usize,
    pub attempted: bool,
    pub solved: bool,
    pub failed: bool,
    pub proof_profile: &'a ProofProfile,
}

#[derive(Debug, Serialize)]
pub struct LevelSummary<'a> {
    pub id: &'a str,
    pub title: &'a str,
    pub tier_id: &'a str,
    pub number: usize,
    pub unlocked: bool,
    pub attempted: bool,
    pub solved: bool,
    pub failed: bool,
    pub width: usize,
    pub height: usize,
    pub mines: usize,
}

#[derive(Debug, Serialize)]
pub struct Symbols {
    pub covered: char,
    pub flag: char,
    pub empty: char,
    pub detonated: char,
    pub numbers: &'static str,
}

impl<'a> Session<'a> {
    pub fn new(campaign: &'a Campaign) -> Self {
        Self {
            campaign,
            active: None,
            attempted: BTreeSet::new(),
            solved: BTreeSet::new(),
        }
    }

    pub fn select(&mut self, level_id: &str) -> Result<(), String> {
        let (tier_index, level_index, level) = self
            .campaign
            .find_level(level_id)
            .ok_or_else(|| format!("unknown level: {level_id}"))?;
        if self.complete() {
            return Err("the campaign is complete".to_owned());
        }
        if self.attempted.contains(level_id) {
            return Err(format!("level {level_id} has already been attempted"));
        }
        if self.current_tier() != Some(tier_index) {
            return Err(format!(
                "tier {} is not the active difficulty",
                self.campaign.tiers[tier_index].id
            ));
        }
        if self.active.as_ref().is_some_and(|active| {
            active.board.snapshot().actions > 0
                && matches!(
                    active.board.status(),
                    GameStatus::Ready | GameStatus::Active
                )
        }) {
            return Err("finish the current one-shot level before selecting another".to_owned());
        }
        self.active = Some(Active {
            tier_index,
            level_index,
            board: Board::new(level),
        });
        Ok(())
    }

    pub fn reveal(&mut self, cells: &[Cell]) -> Result<(Vec<ActionResult>, bool), String> {
        let results = self.active_board_mut()?.reveal(cells)?;
        let newly_passed_tier = self.record_terminal_outcome()?;
        Ok((results, newly_passed_tier))
    }

    pub fn flag(&mut self, cell: Cell) -> Result<ActionResult, String> {
        self.active_board_mut()?.toggle_flag(cell)
    }

    pub fn chord(&mut self, cell: Cell) -> Result<(ActionResult, bool), String> {
        let result = self.active_board_mut()?.chord(cell)?;
        let newly_passed_tier = self.record_terminal_outcome()?;
        Ok((result, newly_passed_tier))
    }

    pub fn levels(&self, tier_filter: Option<&str>) -> Result<Vec<LevelSummary<'_>>, String> {
        if let Some(tier_id) = tier_filter
            && !self.campaign.tiers.iter().any(|tier| tier.id == tier_id)
        {
            return Err(format!("unknown tier: {tier_id}"));
        }
        Ok(self
            .campaign
            .tiers
            .iter()
            .enumerate()
            .filter(|(_, tier)| tier_filter.is_none_or(|id| id == tier.id))
            .flat_map(|(tier_index, tier)| {
                let tier_active = self.current_tier() == Some(tier_index);
                tier.levels
                    .iter()
                    .enumerate()
                    .map(move |(level_index, level)| LevelSummary {
                        id: &level.id,
                        title: &level.title,
                        tier_id: &tier.id,
                        number: level_index + 1,
                        unlocked: tier_active && !self.attempted.contains(&level.id),
                        attempted: self.attempted.contains(&level.id),
                        solved: self.solved.contains(&level.id),
                        failed: self.attempted.contains(&level.id)
                            && !self.solved.contains(&level.id),
                        width: level.width,
                        height: level.height,
                        mines: level.mines,
                    })
            })
            .collect())
    }

    pub fn snapshot(&self) -> Snapshot<'_> {
        let tiers = self
            .campaign
            .tiers
            .iter()
            .enumerate()
            .map(|(index, tier)| {
                let solved = self.solved_in_tier(index);
                let attempted = self.attempted_in_tier(index);
                let wins_required = self.campaign.wins_required(index);
                let status = self.tier_status(index);
                TierSnapshot {
                    id: &tier.id,
                    title: &tier.title,
                    difficulty: &tier.difficulty,
                    status,
                    solved,
                    failed: attempted - solved,
                    attempted,
                    total: tier.levels.len(),
                    wins_required,
                    wins_remaining: wins_required.saturating_sub(solved),
                    attempts_remaining: tier.levels.len() - attempted,
                    opening_distribution: &tier.opening_distribution,
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
                    tier_id: &tier.id,
                    tier_title: &tier.title,
                    number: active.level_index + 1,
                    attempted: self.attempted.contains(&level.id),
                    solved: self.solved.contains(&level.id),
                    failed: self.attempted.contains(&level.id) && !self.solved.contains(&level.id),
                    proof_profile: &level.proof,
                }),
                Some(active.board.snapshot()),
            )
        });
        Snapshot {
            schema: "minesweeper-state-v4",
            guarantee: Guarantee {
                first_click_safe: true,
                safe_radius: 1,
                no_guess: true,
                proof_rules: "local-subset-v1",
                difficulty_metric: "proof-profile-v1",
                distribution_metric: "tier-opening-distribution-v1",
            },
            campaign: CampaignSnapshot {
                id: &self.campaign.id,
                title: &self.campaign.title,
                score: self.score(),
                max_score: self.campaign.max_score(),
                complete: self.complete(),
                status: if !self.complete() {
                    "active"
                } else if self.score() == self.campaign.max_score() {
                    "complete"
                } else {
                    "failed"
                },
            },
            tiers,
            level,
            board,
            symbols: Symbols {
                covered: '?',
                flag: 'F',
                empty: '.',
                detonated: 'X',
                numbers: "12345678",
            },
        }
    }

    pub fn score(&self) -> usize {
        (0..self.campaign.tiers.len())
            .filter(|&index| self.tier_status(index) == "passed")
            .count()
    }

    fn active_level_id(&self) -> Result<&str, String> {
        let active = self
            .active
            .as_ref()
            .ok_or_else(|| "select a level before playing".to_owned())?;
        Ok(&self.campaign.tiers[active.tier_index].levels[active.level_index].id)
    }

    fn active_board(&self) -> Result<&Board, String> {
        self.active
            .as_ref()
            .map(|active| &active.board)
            .ok_or_else(|| "select a level before playing".to_owned())
    }

    fn active_board_mut(&mut self) -> Result<&mut Board, String> {
        self.active
            .as_mut()
            .map(|active| &mut active.board)
            .ok_or_else(|| "select a level before playing".to_owned())
    }

    fn record_terminal_outcome(&mut self) -> Result<bool, String> {
        let status = self.active_board()?.status();
        if !matches!(status, GameStatus::Won | GameStatus::Lost) {
            return Ok(false);
        }
        let level_id = self.active_level_id()?.to_owned();
        let tier_index = self.active.as_ref().expect("active board").tier_index;
        let score_before = self.score();
        if !self.attempted.insert(level_id.clone()) {
            return Err(format!("level {level_id} was already attempted"));
        }
        if status == GameStatus::Won {
            self.solved.insert(level_id);
        }
        Ok(self.score() > score_before && self.tier_status(tier_index) == "passed")
    }

    fn solved_in_tier(&self, tier_index: usize) -> usize {
        self.campaign.tiers[tier_index]
            .levels
            .iter()
            .filter(|level| self.solved.contains(&level.id))
            .count()
    }

    fn attempted_in_tier(&self, tier_index: usize) -> usize {
        self.campaign.tiers[tier_index]
            .levels
            .iter()
            .filter(|level| self.attempted.contains(&level.id))
            .count()
    }

    fn tier_status(&self, tier_index: usize) -> &'static str {
        let solved = self.solved_in_tier(tier_index);
        let attempted = self.attempted_in_tier(tier_index);
        let tier = &self.campaign.tiers[tier_index];
        let wins_required = self.campaign.wins_required(tier_index);
        if solved >= wins_required {
            "passed"
        } else if solved + tier.levels.len() - attempted < wins_required {
            "failed"
        } else if tier_index == 0 || self.tier_status(tier_index - 1) == "passed" {
            "active"
        } else {
            "locked"
        }
    }

    fn current_tier(&self) -> Option<usize> {
        (0..self.campaign.tiers.len()).find(|&index| self.tier_status(index) == "active")
    }

    fn complete(&self) -> bool {
        self.current_tier().is_none()
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    #[test]
    fn five_one_shot_wins_pass_a_tier_and_score_one_point() {
        let campaign = Campaign::load(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("data/campaign/minesweeper.json"),
        )
        .expect("campaign");
        let mut session = Session::new(&campaign);
        assert_eq!(session.snapshot().tiers[1].status, "locked");
        for id in ["cadet-01", "cadet-02", "cadet-03", "cadet-04", "cadet-05"] {
            session.select(id).expect("select");
            let first = Cell { row: 0, column: 0 };
            session.reveal(&[first]).expect("first reveal");
            let mines = session
                .active_board()
                .expect("board")
                .generated_mines()
                .expect("generated")
                .to_vec();
            let cells = mines
                .iter()
                .enumerate()
                .filter(|(_, mine)| !**mine)
                .map(|(index, _)| Cell {
                    row: index / session.active_board().expect("board").snapshot().width,
                    column: index % session.active_board().expect("board").snapshot().width,
                })
                .collect::<Vec<_>>();
            for chunk in cells.chunks(64) {
                if session.active_board().expect("board").status() == GameStatus::Won {
                    break;
                }
                session.reveal(chunk).expect("solve");
            }
        }
        assert_eq!(session.score(), 1);
        assert_eq!(session.snapshot().tiers[0].status, "passed");
        assert_eq!(session.snapshot().tiers[1].status, "active");
        assert_eq!(session.snapshot().campaign.max_score, 5);
    }

    #[test]
    fn a_lost_level_cannot_be_retried_and_six_losses_end_the_campaign() {
        let campaign = Campaign::load(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("data/campaign/minesweeper.json"),
        )
        .expect("campaign");
        let mut session = Session::new(&campaign);
        for id in [
            "cadet-01", "cadet-02", "cadet-03", "cadet-04", "cadet-05", "cadet-06",
        ] {
            session.select(id).expect("select");
            session
                .reveal(&[Cell { row: 0, column: 0 }])
                .expect("first reveal");
            let mine = session
                .active_board()
                .expect("board")
                .generated_mines()
                .expect("generated")
                .iter()
                .position(|value| *value)
                .expect("mine");
            let width = session.active_board().expect("board").snapshot().width;
            session
                .reveal(&[Cell {
                    row: mine / width,
                    column: mine % width,
                }])
                .expect("lose");
            assert!(session.select(id).is_err());
        }
        let snapshot = session.snapshot();
        assert_eq!(snapshot.campaign.score, 0);
        assert_eq!(snapshot.campaign.status, "failed");
        assert!(snapshot.campaign.complete);
        assert_eq!(snapshot.tiers[0].status, "failed");
        assert_eq!(snapshot.tiers[0].attempted, 6);
        assert_eq!(snapshot.tiers[0].failed, 6);
        assert!(session.select("cadet-07").is_err());
    }
}
