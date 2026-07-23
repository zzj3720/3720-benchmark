use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::Level;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    Up,
    Down,
    Left,
    Right,
}

impl Direction {
    pub fn parse(value: &str) -> Result<Self, String> {
        match value.to_ascii_lowercase().as_str() {
            "up" | "u" | "w" => Ok(Self::Up),
            "down" | "d" | "s" => Ok(Self::Down),
            "left" | "l" | "a" => Ok(Self::Left),
            "right" | "r" => Ok(Self::Right),
            _ => Err(format!("unknown direction: {value}")),
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Up => "up",
            Self::Down => "down",
            Self::Left => "left",
            Self::Right => "right",
        }
    }

    const fn delta(self) -> (isize, isize) {
        match self {
            Self::Up => (-1, 0),
            Self::Down => (1, 0),
            Self::Left => (0, -1),
            Self::Right => (0, 1),
        }
    }
}

#[derive(Clone, Debug)]
struct Position {
    player: (usize, usize),
    boxes: BTreeSet<(usize, usize)>,
    pushes: usize,
}

#[derive(Clone, Debug)]
pub struct Board {
    width: usize,
    height: usize,
    walls: BTreeSet<(usize, usize)>,
    goals: BTreeSet<(usize, usize)>,
    position: Position,
    initial: Position,
    history: Vec<Position>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct StepResult {
    pub direction: &'static str,
    pub moved: bool,
    pub pushed: bool,
    pub solved: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct BoardSnapshot {
    pub width: usize,
    pub height: usize,
    pub map: Vec<String>,
    pub player: Coordinate,
    pub boxes: Vec<Coordinate>,
    pub goals: Vec<Coordinate>,
    pub boxes_on_goals: usize,
    pub moves: usize,
    pub pushes: usize,
    pub solved: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct Coordinate {
    pub row: usize,
    pub column: usize,
}

impl Board {
    pub fn from_level(level: &Level) -> Self {
        let mut walls = BTreeSet::new();
        let mut goals = BTreeSet::new();
        let mut boxes = BTreeSet::new();
        let mut player = None;
        for (row, line) in level.rows.iter().enumerate() {
            for (column, tile) in line.chars().enumerate() {
                let coordinate = (row, column);
                match tile {
                    '#' => {
                        walls.insert(coordinate);
                    }
                    '.' => {
                        goals.insert(coordinate);
                    }
                    '$' => {
                        boxes.insert(coordinate);
                    }
                    '*' => {
                        boxes.insert(coordinate);
                        goals.insert(coordinate);
                    }
                    '@' => player = Some(coordinate),
                    '+' => {
                        player = Some(coordinate);
                        goals.insert(coordinate);
                    }
                    ' ' => {}
                    _ => unreachable!("campaign validation rejects unsupported tiles"),
                }
            }
        }
        let initial = Position {
            player: player.expect("campaign validation requires a player"),
            boxes,
            pushes: 0,
        };
        Self {
            width: level.width,
            height: level.height,
            walls,
            goals,
            position: initial.clone(),
            initial,
            history: Vec::new(),
        }
    }

    pub fn step(&mut self, direction: Direction) -> StepResult {
        let (dr, dc) = direction.delta();
        let Some(next) = offset(self.position.player, dr, dc, self.height, self.width) else {
            return self.blocked(direction);
        };
        if self.walls.contains(&next) {
            return self.blocked(direction);
        }
        let pushed = self.position.boxes.contains(&next);
        let box_target = pushed
            .then(|| offset(next, dr, dc, self.height, self.width))
            .flatten();
        if pushed
            && box_target.is_none_or(|target| {
                self.walls.contains(&target) || self.position.boxes.contains(&target)
            })
        {
            return self.blocked(direction);
        }

        self.history.push(self.position.clone());
        self.position.player = next;
        if let Some(target) = box_target {
            self.position.boxes.remove(&next);
            self.position.boxes.insert(target);
            self.position.pushes += 1;
        }
        StepResult {
            direction: direction.as_str(),
            moved: true,
            pushed,
            solved: self.solved(),
        }
    }

    pub fn undo(&mut self, steps: usize) -> usize {
        let mut undone = 0;
        for _ in 0..steps {
            let Some(position) = self.history.pop() else {
                break;
            };
            self.position = position;
            undone += 1;
        }
        undone
    }

    pub fn reset(&mut self) {
        self.position = self.initial.clone();
        self.history.clear();
    }

    pub fn solved(&self) -> bool {
        self.position.boxes == self.goals
    }

    pub fn snapshot(&self) -> BoardSnapshot {
        let mut map = Vec::with_capacity(self.height);
        for row in 0..self.height {
            let mut line = String::with_capacity(self.width);
            for column in 0..self.width {
                let coordinate = (row, column);
                let tile = if self.walls.contains(&coordinate) {
                    '#'
                } else if self.position.player == coordinate {
                    if self.goals.contains(&coordinate) {
                        '+'
                    } else {
                        '@'
                    }
                } else if self.position.boxes.contains(&coordinate) {
                    if self.goals.contains(&coordinate) {
                        '*'
                    } else {
                        '$'
                    }
                } else if self.goals.contains(&coordinate) {
                    '.'
                } else {
                    ' '
                };
                line.push(tile);
            }
            map.push(line);
        }
        BoardSnapshot {
            width: self.width,
            height: self.height,
            map,
            player: coordinate(self.position.player),
            boxes: self
                .position
                .boxes
                .iter()
                .copied()
                .map(coordinate)
                .collect(),
            goals: self.goals.iter().copied().map(coordinate).collect(),
            boxes_on_goals: self.position.boxes.intersection(&self.goals).count(),
            moves: self.history.len(),
            pushes: self.position.pushes,
            solved: self.solved(),
        }
    }

    fn blocked(&self, direction: Direction) -> StepResult {
        StepResult {
            direction: direction.as_str(),
            moved: false,
            pushed: false,
            solved: self.solved(),
        }
    }
}

fn offset(
    (row, column): (usize, usize),
    dr: isize,
    dc: isize,
    height: usize,
    width: usize,
) -> Option<(usize, usize)> {
    let row = row.checked_add_signed(dr)?;
    let column = column.checked_add_signed(dc)?;
    (row < height && column < width).then_some((row, column))
}

fn coordinate((row, column): (usize, usize)) -> Coordinate {
    Coordinate { row, column }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn level(rows: &[&str]) -> Level {
        Level {
            id: "test".into(),
            source_id: "test".into(),
            title: "Test".into(),
            width: rows[0].len(),
            height: rows.len(),
            rows: rows.iter().map(|row| (*row).to_owned()).collect(),
        }
    }

    #[test]
    fn pushes_solves_undoes_and_resets() {
        let mut board = Board::from_level(&level(&["#####", "#@$.#", "#####"]));
        let result = board.step(Direction::Right);
        assert!(result.moved);
        assert!(result.pushed);
        assert!(result.solved);
        assert_eq!(board.snapshot().pushes, 1);
        assert_eq!(board.undo(1), 1);
        assert!(!board.solved());
        board.step(Direction::Right);
        board.reset();
        assert_eq!(board.snapshot().moves, 0);
        assert_eq!(board.snapshot().pushes, 0);
    }

    #[test]
    fn walls_and_double_boxes_block_without_counting_a_move() {
        let mut wall = Board::from_level(&level(&["#####", "#@$.#", "#####"]));
        assert!(!wall.step(Direction::Up).moved);
        assert_eq!(wall.snapshot().moves, 0);

        let mut boxes = Board::from_level(&level(&["######", "#@$$.#", "# .. #", "######"]));
        assert!(!boxes.step(Direction::Right).moved);
        assert_eq!(boxes.snapshot().moves, 0);
    }
}
