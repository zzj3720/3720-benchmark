use std::fs;
use std::path::Path;

use crate::model::Direction;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Replay {
    pub directions: Vec<Direction>,
    pub removed_undos: usize,
}

impl Replay {
    pub fn load(path: impl AsRef<Path>) -> Result<Self, String> {
        let source = fs::read_to_string(path.as_ref())
            .map_err(|error| format!("could not read {}: {error}", path.as_ref().display()))?;
        Self::parse(&source)
    }

    pub fn parse(source: &str) -> Result<Self, String> {
        let mut directions = Vec::new();
        let mut removed_undos = 0;
        for (line_index, line) in source.lines().enumerate() {
            let value = line.trim();
            if value.is_empty() {
                continue;
            }
            if value.eq_ignore_ascii_case("undo") {
                directions.pop().ok_or_else(|| {
                    format!(
                        "undo without a preceding direction on line {}",
                        line_index + 1
                    )
                })?;
                removed_undos += 1;
                continue;
            }
            directions.push(
                Direction::parse(value)
                    .map_err(|error| format!("invalid replay line {}: {error}", line_index + 1))?,
            );
        }
        Ok(Self {
            directions,
            removed_undos,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn removes_the_move_preceding_each_tas_undo() {
        let replay = Replay::parse("North\r\nEast\r\nUndo\r\nWest\r\n").expect("valid replay");
        assert_eq!(replay.directions, vec![Direction::North, Direction::West]);
        assert_eq!(replay.removed_undos, 1);
    }

    #[test]
    fn imports_the_complete_tas() {
        let path = crate::data_root().join("oracle").join("all.dem");
        let replay = Replay::load(path).expect("TAS should parse");
        assert_eq!(replay.directions.len(), 16_361);
        assert_eq!(replay.removed_undos, 103);
    }
}
