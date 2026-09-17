use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::{Level, ProofProfile};

const MAX_GENERATION_ATTEMPTS: u32 = 1_000;

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Cell {
    pub row: usize,
    pub column: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GameStatus {
    Ready,
    Active,
    Won,
    Lost,
}

#[derive(Clone, Debug)]
struct Generated {
    mines: Vec<bool>,
    adjacent: Vec<u8>,
}

#[derive(Clone, Debug)]
pub struct Board {
    level: Level,
    generated: Option<Generated>,
    revealed: Vec<bool>,
    flagged: Vec<bool>,
    status: GameStatus,
    first_reveal: Option<Cell>,
    generation_attempts: u32,
    proof: Option<ProofMetrics>,
    actions: usize,
}

#[derive(Clone, Debug, Serialize)]
pub struct ActionResult {
    pub action: &'static str,
    pub cell: Cell,
    pub changed: usize,
    pub status: GameStatus,
}

#[derive(Clone, Debug, Serialize)]
pub struct BoardSnapshot {
    pub width: usize,
    pub height: usize,
    pub mines: usize,
    pub status: GameStatus,
    pub first_reveal: Option<Cell>,
    pub generation_attempts: u32,
    pub proof: Option<ProofMetrics>,
    pub actions: usize,
    pub revealed: usize,
    pub flagged: usize,
    pub remaining_safe: usize,
    pub map: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ProofMetrics {
    pub opening_revealed: usize,
    pub safe_cells: usize,
    pub proof_rounds: usize,
    pub subset_rounds: usize,
    pub subset_cells: usize,
    pub max_frontier: usize,
}

impl ProofMetrics {
    fn meets(&self, profile: &ProofProfile) -> bool {
        self.opening_revealed * 100 >= profile.min_opening_percent * self.safe_cells
            && self.opening_revealed * 100 <= profile.max_opening_percent * self.safe_cells
            && self.proof_rounds >= profile.min_proof_rounds
            && self.subset_rounds >= profile.min_subset_rounds
            && self.max_frontier >= profile.min_frontier
    }
}

impl Board {
    pub fn new(level: &Level) -> Self {
        let cells = level.width * level.height;
        Self {
            level: level.clone(),
            generated: None,
            revealed: vec![false; cells],
            flagged: vec![false; cells],
            status: GameStatus::Ready,
            first_reveal: None,
            generation_attempts: 0,
            proof: None,
            actions: 0,
        }
    }

    pub fn reveal(&mut self, cells: &[Cell]) -> Result<Vec<ActionResult>, String> {
        if cells.is_empty() || cells.len() > 64 {
            return Err("reveal accepts between 1 and 64 cells".to_owned());
        }
        self.ensure_playable()?;
        for &cell in cells {
            self.validate_cell(cell)?;
            if self.flagged[self.index(cell)] {
                return Err(format!(
                    "cannot reveal flagged cell {},{}",
                    cell.row, cell.column
                ));
            }
        }
        let mut results = Vec::with_capacity(cells.len());
        for &cell in cells {
            if self.generated.is_none() {
                let (generated, attempts, proof) = generate_verified(&self.level, cell)?;
                self.generated = Some(generated);
                self.first_reveal = Some(cell);
                self.generation_attempts = attempts;
                self.proof = Some(proof);
                self.status = GameStatus::Active;
            }
            let changed = self.reveal_from(cell);
            self.actions += 1;
            self.update_win();
            results.push(ActionResult {
                action: "reveal",
                cell,
                changed,
                status: self.status,
            });
            if matches!(self.status, GameStatus::Won | GameStatus::Lost) {
                break;
            }
        }
        Ok(results)
    }

    pub fn toggle_flag(&mut self, cell: Cell) -> Result<ActionResult, String> {
        self.ensure_playable()?;
        self.validate_cell(cell)?;
        let index = self.index(cell);
        if self.revealed[index] {
            return Err(format!(
                "cannot flag revealed cell {},{}",
                cell.row, cell.column
            ));
        }
        self.flagged[index] = !self.flagged[index];
        self.actions += 1;
        Ok(ActionResult {
            action: if self.flagged[index] {
                "flag"
            } else {
                "unflag"
            },
            cell,
            changed: 1,
            status: self.status,
        })
    }

    pub fn chord(&mut self, cell: Cell) -> Result<ActionResult, String> {
        self.ensure_playable()?;
        self.validate_cell(cell)?;
        let index = self.index(cell);
        let generated = self
            .generated
            .as_ref()
            .ok_or_else(|| "reveal a cell before chording".to_owned())?;
        if !self.revealed[index] || generated.adjacent[index] == 0 {
            return Err("chord requires a revealed numbered cell".to_owned());
        }
        let neighbors = self.neighbors(cell);
        let flags = neighbors
            .iter()
            .filter(|&&neighbor| self.flagged[self.index(neighbor)])
            .count();
        if flags != generated.adjacent[index] as usize {
            return Err(format!(
                "chord needs {} adjacent flags, found {flags}",
                generated.adjacent[index]
            ));
        }
        let mut changed = 0;
        for neighbor in neighbors {
            let neighbor_index = self.index(neighbor);
            if !self.flagged[neighbor_index] && !self.revealed[neighbor_index] {
                changed += self.reveal_from(neighbor);
                if self.status == GameStatus::Lost {
                    break;
                }
            }
        }
        self.actions += 1;
        self.update_win();
        Ok(ActionResult {
            action: "chord",
            cell,
            changed,
            status: self.status,
        })
    }

    pub fn snapshot(&self) -> BoardSnapshot {
        let revealed = self
            .revealed
            .iter()
            .enumerate()
            .filter(|(index, value)| {
                **value
                    && self
                        .generated
                        .as_ref()
                        .is_none_or(|generated| !generated.mines[*index])
            })
            .count();
        BoardSnapshot {
            width: self.level.width,
            height: self.level.height,
            mines: self.level.mines,
            status: self.status,
            first_reveal: self.first_reveal,
            generation_attempts: self.generation_attempts,
            proof: self.proof.clone(),
            actions: self.actions,
            revealed,
            flagged: self.flagged.iter().filter(|&&value| value).count(),
            remaining_safe: self.level.width * self.level.height - self.level.mines - revealed,
            map: (0..self.level.height)
                .map(|row| {
                    (0..self.level.width)
                        .map(|column| self.visible_char(Cell { row, column }))
                        .collect()
                })
                .collect(),
        }
    }

    pub fn status(&self) -> GameStatus {
        self.status
    }

    pub fn generated_mines(&self) -> Option<&[bool]> {
        self.generated.as_ref().map(|board| board.mines.as_slice())
    }

    fn ensure_playable(&self) -> Result<(), String> {
        if matches!(self.status, GameStatus::Won | GameStatus::Lost) {
            Err("the level is terminal; select another unattempted level".to_owned())
        } else {
            Ok(())
        }
    }

    fn validate_cell(&self, cell: Cell) -> Result<(), String> {
        if cell.row >= self.level.height || cell.column >= self.level.width {
            Err(format!(
                "cell {},{} is outside the {}x{} board",
                cell.row, cell.column, self.level.height, self.level.width
            ))
        } else {
            Ok(())
        }
    }

    fn index(&self, cell: Cell) -> usize {
        cell.row * self.level.width + cell.column
    }

    fn neighbors(&self, cell: Cell) -> Vec<Cell> {
        neighbors(self.level.width, self.level.height, cell)
    }

    fn reveal_from(&mut self, cell: Cell) -> usize {
        let start = self.index(cell);
        let generated = self.generated.as_ref().expect("board generated");
        if generated.mines[start] {
            self.revealed[start] = true;
            self.status = GameStatus::Lost;
            return 1;
        }
        reveal_safe_region(
            self.level.width,
            self.level.height,
            generated,
            &mut self.revealed,
            &self.flagged,
            cell,
        )
    }

    fn update_win(&mut self) {
        if self.status != GameStatus::Lost
            && self.revealed.iter().filter(|&&value| value).count()
                == self.level.width * self.level.height - self.level.mines
        {
            self.status = GameStatus::Won;
        }
    }

    fn visible_char(&self, cell: Cell) -> char {
        let index = self.index(cell);
        if self.flagged[index] {
            return 'F';
        }
        if !self.revealed[index] {
            return '?';
        }
        let generated = self
            .generated
            .as_ref()
            .expect("revealed board is generated");
        if generated.mines[index] {
            return 'X';
        }
        match generated.adjacent[index] {
            0 => '.',
            value => char::from(b'0' + value),
        }
    }
}

fn generate_verified(level: &Level, first: Cell) -> Result<(Generated, u32, ProofMetrics), String> {
    let allowed = (0..level.height)
        .flat_map(|row| (0..level.width).map(move |column| Cell { row, column }))
        .filter(|cell| cell.row.abs_diff(first.row) > 1 || cell.column.abs_diff(first.column) > 1)
        .map(|cell| cell.row * level.width + cell.column)
        .collect::<Vec<_>>();
    if allowed.len() < level.mines {
        return Err("not enough cells outside the first-click safety area".to_owned());
    }
    for attempt in 1..=MAX_GENERATION_ATTEMPTS {
        let seed = mix_seed(level.seed, first, attempt);
        let mut candidates = allowed.clone();
        shuffle(&mut candidates, seed);
        let mut mines = vec![false; level.width * level.height];
        for &index in candidates.iter().take(level.mines) {
            mines[index] = true;
        }
        let generated = Generated {
            adjacent: adjacent_counts(level.width, level.height, &mines),
            mines,
        };
        if let Some(proof) = logic_proof(level.width, level.height, &generated, first)
            && proof.meets(&level.proof)
        {
            return Ok((generated, attempt, proof));
        }
    }
    Err(format!(
        "could not generate a no-guess board meeting the proof profile for {} after {MAX_GENERATION_ATTEMPTS} attempts",
        level.id
    ))
}

fn mix_seed(level_seed: u64, first: Cell, attempt: u32) -> u64 {
    let coordinate = ((first.row as u64) << 32) | first.column as u64;
    level_seed
        ^ coordinate.wrapping_mul(0x9e37_79b9_7f4a_7c15)
        ^ (attempt as u64).wrapping_mul(0xbf58_476d_1ce4_e5b9)
}

fn shuffle(values: &mut [usize], mut state: u64) {
    for upper in (1..values.len()).rev() {
        state = splitmix64(state);
        values.swap(upper, state as usize % (upper + 1));
    }
}

fn splitmix64(mut value: u64) -> u64 {
    value = value.wrapping_add(0x9e37_79b9_7f4a_7c15);
    value = (value ^ (value >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    value = (value ^ (value >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    value ^ (value >> 31)
}

fn adjacent_counts(width: usize, height: usize, mines: &[bool]) -> Vec<u8> {
    (0..width * height)
        .map(|index| {
            let cell = Cell {
                row: index / width,
                column: index % width,
            };
            neighbors(width, height, cell)
                .iter()
                .filter(|cell| mines[cell.row * width + cell.column])
                .count() as u8
        })
        .collect()
}

fn neighbors(width: usize, height: usize, cell: Cell) -> Vec<Cell> {
    let mut values = Vec::with_capacity(8);
    for row in cell.row.saturating_sub(1)..=(cell.row + 1).min(height - 1) {
        for column in cell.column.saturating_sub(1)..=(cell.column + 1).min(width - 1) {
            if row != cell.row || column != cell.column {
                values.push(Cell { row, column });
            }
        }
    }
    values
}

fn reveal_safe_region(
    width: usize,
    height: usize,
    generated: &Generated,
    revealed: &mut [bool],
    blocked: &[bool],
    start: Cell,
) -> usize {
    let mut stack = vec![start];
    let mut changed = 0;
    while let Some(cell) = stack.pop() {
        let index = cell.row * width + cell.column;
        if revealed[index] || blocked[index] || generated.mines[index] {
            continue;
        }
        revealed[index] = true;
        changed += 1;
        if generated.adjacent[index] == 0 {
            stack.extend(neighbors(width, height, cell));
        }
    }
    changed
}

#[derive(Clone)]
struct Constraint {
    unknown: BTreeSet<usize>,
    mines: usize,
}

fn logic_proof(
    width: usize,
    height: usize,
    generated: &Generated,
    first: Cell,
) -> Option<ProofMetrics> {
    let cells = width * height;
    let mut revealed = vec![false; cells];
    let mut known_mines = vec![false; cells];
    let opening_revealed = reveal_safe_region(
        width,
        height,
        generated,
        &mut revealed,
        &vec![false; cells],
        first,
    );
    let mut proof = ProofMetrics {
        opening_revealed,
        safe_cells: cells - generated.mines.iter().filter(|&&mine| mine).count(),
        proof_rounds: 0,
        subset_rounds: 0,
        subset_cells: 0,
        max_frontier: 0,
    };
    loop {
        if (0..cells).all(|index| generated.mines[index] || revealed[index]) {
            return Some(proof);
        }
        let constraints = revealed
            .iter()
            .enumerate()
            .filter(|(_, value)| **value)
            .filter_map(|(index, _)| {
                let cell = Cell {
                    row: index / width,
                    column: index % width,
                };
                let adjacent = neighbors(width, height, cell);
                let known = adjacent
                    .iter()
                    .filter(|cell| known_mines[cell.row * width + cell.column])
                    .count();
                let unknown = adjacent
                    .iter()
                    .map(|cell| cell.row * width + cell.column)
                    .filter(|&neighbor| !revealed[neighbor] && !known_mines[neighbor])
                    .collect::<BTreeSet<_>>();
                (!unknown.is_empty()).then_some(Constraint {
                    unknown,
                    mines: generated.adjacent[index] as usize - known,
                })
            })
            .collect::<Vec<_>>();
        proof.max_frontier = proof.max_frontier.max(
            constraints
                .iter()
                .flat_map(|constraint| &constraint.unknown)
                .copied()
                .collect::<BTreeSet<_>>()
                .len(),
        );
        let mut direct_safe = BTreeSet::new();
        let mut direct_mines = BTreeSet::new();
        for constraint in &constraints {
            if constraint.mines == 0 {
                direct_safe.extend(&constraint.unknown);
            } else if constraint.mines == constraint.unknown.len() {
                direct_mines.extend(&constraint.unknown);
            }
        }
        let mut subset_safe = BTreeSet::new();
        let mut subset_mines = BTreeSet::new();
        for left in &constraints {
            for right in &constraints {
                if left.unknown != right.unknown && left.unknown.is_subset(&right.unknown) {
                    let difference = right
                        .unknown
                        .difference(&left.unknown)
                        .copied()
                        .collect::<BTreeSet<_>>();
                    let remaining = right.mines.saturating_sub(left.mines);
                    if remaining == 0 {
                        subset_safe.extend(difference);
                    } else if remaining == difference.len() {
                        subset_mines.extend(difference);
                    }
                }
            }
        }
        let subset_cells = subset_safe
            .union(&subset_mines)
            .copied()
            .filter(|index| !direct_safe.contains(index) && !direct_mines.contains(index))
            .count();
        if subset_cells > 0 {
            proof.subset_rounds += 1;
            proof.subset_cells += subset_cells;
        }
        let mut safe = direct_safe;
        safe.extend(subset_safe);
        let mut mines = direct_mines;
        mines.extend(subset_mines);
        safe.retain(|index| !mines.contains(index));
        if safe.is_empty() && mines.is_empty() {
            return None;
        }
        proof.proof_rounds += 1;
        for index in mines {
            known_mines[index] = true;
        }
        for index in safe {
            reveal_safe_region(
                width,
                height,
                generated,
                &mut revealed,
                &known_mines,
                Cell {
                    row: index / width,
                    column: index % width,
                },
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;
    use crate::Campaign;

    fn level() -> Level {
        Level {
            id: "test".into(),
            title: "Test".into(),
            width: 8,
            height: 8,
            mines: 10,
            seed: 3720,
            proof: ProofProfile {
                min_opening_percent: 1,
                max_opening_percent: 100,
                min_proof_rounds: 0,
                min_subset_rounds: 0,
                min_frontier: 0,
            },
        }
    }

    #[test]
    fn every_first_click_is_safe_and_logically_solvable() {
        let level = level();
        for row in 0..level.height {
            for column in 0..level.width {
                let first = Cell { row, column };
                let (generated, _, proof) =
                    generate_verified(&level, first).expect("generated board");
                assert!(!generated.mines[row * level.width + column]);
                assert!(
                    neighbors(level.width, level.height, first)
                        .iter()
                        .all(|cell| !generated.mines[cell.row * level.width + cell.column])
                );
                assert_eq!(
                    logic_proof(level.width, level.height, &generated, first),
                    Some(proof)
                );
            }
        }
    }

    #[test]
    fn every_campaign_first_click_meets_the_frozen_proof_profile() {
        let campaign = Campaign::load(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("data/campaign/minesweeper.json"),
        )
        .expect("campaign");
        let mut clicks = 0;
        let mut maximum_attempts = 0;
        for tier in &campaign.tiers {
            let mut tier_proofs = Vec::new();
            let mut tier_attempts = Vec::new();
            for level in &tier.levels {
                for row in 0..level.height {
                    for column in 0..level.width {
                        let first = Cell { row, column };
                        let (generated, attempts, proof) =
                            generate_verified(level, first).expect("verified board");
                        clicks += 1;
                        maximum_attempts = maximum_attempts.max(attempts);
                        assert!(!generated.mines[row * level.width + column]);
                        assert!(
                            neighbors(level.width, level.height, first)
                                .iter()
                                .all(|cell| !generated.mines[cell.row * level.width + cell.column])
                        );
                        assert!(proof.meets(&level.proof));
                        assert_eq!(
                            logic_proof(level.width, level.height, &generated, first),
                            Some(proof.clone())
                        );
                        tier_proofs.push(proof);
                        tier_attempts.push(attempts);
                    }
                }
            }
            tier_proofs.sort_by(|left, right| {
                (left.opening_revealed * right.safe_cells)
                    .cmp(&(right.opening_revealed * left.safe_cells))
            });
            tier_attempts.sort_unstable();
            let opening_sum = tier_proofs
                .iter()
                .map(|proof| proof.opening_revealed)
                .sum::<usize>();
            let safe_sum = tier_proofs
                .iter()
                .map(|proof| proof.safe_cells)
                .sum::<usize>();
            let upper_median = &tier_proofs[tier_proofs.len() / 2];
            let distribution = &tier.opening_distribution;
            assert!(
                opening_sum * 100 >= distribution.min_weighted_mean_percent * safe_sum
                    && opening_sum * 100 <= distribution.max_weighted_mean_percent * safe_sum,
                "{} weighted opening mean is outside its frozen band",
                tier.id
            );
            assert!(
                upper_median.opening_revealed * 100
                    >= distribution.min_upper_median_percent * upper_median.safe_cells
                    && upper_median.opening_revealed * 100
                        <= distribution.max_upper_median_percent * upper_median.safe_cells,
                "{} upper opening median is outside its frozen band",
                tier.id
            );
            println!(
                "{}: samples={}, opening min={:.2}%, weighted_mean={:.2}%, upper_median={:.2}%, max={:.2}%, attempts p50/p95/p99/max={}/{}/{}/{}",
                tier.id,
                tier_proofs.len(),
                tier_proofs
                    .first()
                    .map(|proof| proof.opening_revealed as f64 * 100.0 / proof.safe_cells as f64)
                    .expect("proof"),
                opening_sum as f64 * 100.0 / safe_sum as f64,
                upper_median.opening_revealed as f64 * 100.0 / upper_median.safe_cells as f64,
                tier_proofs
                    .last()
                    .map(|proof| proof.opening_revealed as f64 * 100.0 / proof.safe_cells as f64)
                    .expect("proof"),
                tier_attempts[tier_attempts.len() / 2],
                tier_attempts[tier_attempts.len() * 95 / 100],
                tier_attempts[tier_attempts.len() * 99 / 100],
                tier_attempts.last().copied().expect("attempt"),
            );
        }
        assert_eq!(clicks, 7_038);
        assert_eq!(maximum_attempts, 661);
        assert!(maximum_attempts <= MAX_GENERATION_ATTEMPTS);
    }

    #[test]
    fn losing_is_terminal() {
        let level = level();
        let mut board = Board::new(&level);
        board
            .reveal(&[Cell { row: 0, column: 0 }])
            .expect("first reveal");
        assert!(board.snapshot().proof.is_some());
        let mine = board
            .generated_mines()
            .expect("generated")
            .iter()
            .position(|value| *value)
            .expect("mine");
        board
            .reveal(&[Cell {
                row: mine / level.width,
                column: mine % level.width,
            }])
            .expect("mine reveal");
        assert_eq!(board.status(), GameStatus::Lost);
        assert!(board.reveal(&[Cell { row: 0, column: 1 }]).is_err());
    }

    #[test]
    fn a_correct_chord_reveals_only_safe_neighbors() {
        let level = level();
        let mut board = Board::new(&level);
        board
            .reveal(&[Cell { row: 0, column: 0 }])
            .expect("first reveal");
        let snapshot = board.snapshot();
        let mines = board.generated_mines().expect("generated").to_vec();
        let center = (0..level.width * level.height)
            .map(|index| Cell {
                row: index / level.width,
                column: index % level.width,
            })
            .find(|cell| {
                let tile = snapshot.map[cell.row].as_bytes()[cell.column];
                tile.is_ascii_digit()
                    && neighbors(level.width, level.height, *cell)
                        .iter()
                        .any(|neighbor| {
                            snapshot.map[neighbor.row].as_bytes()[neighbor.column] == b'?'
                                && !mines[neighbor.row * level.width + neighbor.column]
                        })
            })
            .expect("revealed frontier number");
        for mine in neighbors(level.width, level.height, center)
            .into_iter()
            .filter(|cell| mines[cell.row * level.width + cell.column])
        {
            board.toggle_flag(mine).expect("flag known mine");
        }
        let result = board.chord(center).expect("correct chord");
        assert!(result.changed > 0);
        assert_ne!(board.status(), GameStatus::Lost);
    }

    #[test]
    fn an_invalid_reveal_batch_is_rejected_before_generation() {
        let level = level();
        let mut board = Board::new(&level);
        let flagged = Cell { row: 4, column: 4 };
        board.toggle_flag(flagged).expect("flag");
        assert!(
            board
                .reveal(&[Cell { row: 0, column: 0 }, flagged])
                .is_err()
        );
        assert_eq!(board.status(), GameStatus::Ready);
        assert_eq!(board.snapshot().first_reveal, None);
    }
}
