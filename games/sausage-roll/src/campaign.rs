use std::collections::HashMap;
use std::fs::File;
use std::io::{self, BufReader, Read};
use std::path::Path;

use flate2::read::GzDecoder;
use serde::{Deserialize, Serialize};

use crate::model::{Coord, Direction, GameState};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct PositionedDirection {
    pub pos: Coord,
    pub direction: Direction,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Temple {
    pub name: String,
    pub levels: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct IslandMask {
    pub dimensions: [usize; 3],
    pub cells: Vec<i32>,
    pub offset: Coord,
}

impl IslandMask {
    pub fn get(&self, local: Coord) -> Option<i32> {
        let [width, height, depth] = self.dimensions;
        let x = usize::try_from(local.x).ok()?;
        let y = usize::try_from(local.y).ok()?;
        let z = usize::try_from(local.z).ok()?;
        if x >= width || y >= height || z >= depth {
            return None;
        }
        self.cells.get(z + depth * y + depth * height * x).copied()
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ProjectionCompatibility {
    pub dimensions: [usize; 2],
    pub cells: Vec<bool>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Campaign {
    pub island_names: Vec<String>,
    pub offsets: HashMap<String, Coord>,
    pub sausage_positions: HashMap<String, Vec<PositionedDirection>>,
    pub player_positions: HashMap<String, PositionedDirection>,
    pub temples: Vec<Temple>,
    pub island_masks: HashMap<String, IslandMask>,
    pub projection_compatibilities: HashMap<String, HashMap<String, ProjectionCompatibility>>,
    pub merged_state_source: String,
    pub island_state_sources: HashMap<String, String>,
}

impl Campaign {
    pub fn load_gzip(path: impl AsRef<Path>) -> Result<Self, String> {
        let file = File::open(path.as_ref())
            .map_err(|error| format!("could not open {}: {error}", path.as_ref().display()))?;
        let decoder = GzDecoder::new(BufReader::new(file));
        Self::read(decoder)
    }

    pub fn read(reader: impl Read) -> Result<Self, String> {
        let mut reader = BinaryReader::new(reader);

        let island_count = reader.count("island count")?;
        let mut island_names = Vec::with_capacity(island_count);
        let mut offsets = HashMap::with_capacity(island_count);
        for _ in 0..island_count {
            let name = reader.string()?;
            let offset = reader.coord()?;
            offsets.insert(name.clone(), offset);
            island_names.push(name);
        }

        let mut sausage_positions = HashMap::new();
        for _ in 0..reader.count("sausage-position island count")? {
            let name = reader.string()?;
            let mut positions = Vec::new();
            for _ in 0..reader.count("sausage-position count")? {
                positions.push(PositionedDirection {
                    pos: reader.coord()?,
                    direction: reader.direction()?,
                });
            }
            sausage_positions.insert(name, positions);
        }

        let mut player_positions = HashMap::new();
        for _ in 0..reader.count("player-position count")? {
            let name = reader.string()?;
            player_positions.insert(
                name,
                PositionedDirection {
                    pos: reader.coord()?,
                    direction: reader.direction()?,
                },
            );
        }

        let mut temples = Vec::new();
        for _ in 0..reader.count("temple count")? {
            let name = reader.string()?;
            let mut levels = Vec::new();
            for _ in 0..reader.count("temple level count")? {
                levels.push(reader.string()?);
            }
            temples.push(Temple { name, levels });
        }

        let mut island_masks = HashMap::new();
        for _ in 0..reader.count("island-mask count")? {
            let name = reader.string()?;
            let dimensions = [
                reader.count("island-mask width")?,
                reader.count("island-mask height")?,
                reader.count("island-mask depth")?,
            ];
            let cell_count = dimensions
                .into_iter()
                .try_fold(1usize, |total, value| total.checked_mul(value))
                .ok_or_else(|| "island-mask dimensions overflow".to_owned())?;
            let mut cells = Vec::with_capacity(cell_count);
            for _ in 0..cell_count {
                cells.push(reader.i32()?);
            }
            island_masks.insert(
                name,
                IslandMask {
                    dimensions,
                    cells,
                    offset: reader.coord()?,
                },
            );
        }

        let mut projection_compatibilities = HashMap::new();
        for _ in 0..reader.count("projection outer count")? {
            let outer_name = reader.string()?;
            let mut inner = HashMap::new();
            for _ in 0..reader.count("projection inner count")? {
                let inner_name = reader.string()?;
                let dimensions = [
                    reader.count("projection width")?,
                    reader.count("projection height")?,
                ];
                let cell_count = dimensions[0]
                    .checked_mul(dimensions[1])
                    .ok_or_else(|| "projection dimensions overflow".to_owned())?;
                let cells = reader
                    .bytes(cell_count)?
                    .into_iter()
                    .map(|cell| cell != 0)
                    .collect();
                inner.insert(inner_name, ProjectionCompatibility { dimensions, cells });
            }
            projection_compatibilities.insert(outer_name, inner);
        }

        skip_coord_maps(&mut reader, "coast data")?;
        skip_coord_maps(&mut reader, "splash data")?;

        for _ in 0..reader.count("bbq island count")? {
            reader.string()?;
            skip_coords(&mut reader, "bbq coordinate count")?;
        }
        for _ in 0..reader.count("tree island count")? {
            reader.string()?;
            for _ in 0..reader.count("tree coordinate count")? {
                reader.coord()?;
                reader.i32()?;
            }
        }

        let merged_state_source = reader.string()?;
        let mut island_state_sources = HashMap::new();
        for _ in 0..reader.count("island state count")? {
            island_state_sources.insert(reader.string()?, reader.string()?);
        }
        reader.expect_eof()?;

        Ok(Self {
            island_names,
            offsets,
            sausage_positions,
            player_positions,
            temples,
            island_masks,
            projection_compatibilities,
            merged_state_source,
            island_state_sources,
        })
    }

    pub fn merged_state(&self) -> Result<GameState, String> {
        GameState::parse(&self.merged_state_source)
    }

    pub fn island_state(&self, name: &str) -> Result<GameState, String> {
        let source = self
            .island_state_sources
            .get(name)
            .ok_or_else(|| format!("unknown island {name:?}"))?;
        GameState::parse(source)
    }

    pub fn puzzle_ids(&self) -> Vec<&str> {
        self.temples
            .iter()
            .flat_map(|temple| temple.levels.iter().map(String::as_str))
            .collect()
    }

    pub fn puzzle_names(&self) -> Result<Vec<String>, String> {
        self.puzzle_ids()
            .into_iter()
            .map(|id| {
                self.island_state_sources
                    .get(id)
                    .ok_or_else(|| format!("missing puzzle state {id:?}"))
                    .and_then(|source| GameState::parse(source))
                    .map(|state| state.display_name)
            })
            .collect()
    }
}

fn skip_coords(reader: &mut BinaryReader<impl Read>, label: &str) -> Result<(), String> {
    for _ in 0..reader.count(label)? {
        reader.coord()?;
    }
    Ok(())
}

fn skip_coord_maps(reader: &mut BinaryReader<impl Read>, label: &str) -> Result<(), String> {
    for _ in 0..reader.count(&format!("{label} island count"))? {
        reader.string()?;
        for _ in 0..reader.count(&format!("{label} group count"))? {
            reader.i32()?;
            skip_coords(reader, &format!("{label} coordinate count"))?;
        }
    }
    Ok(())
}

struct BinaryReader<R> {
    inner: R,
    offset: usize,
}

impl<R: Read> BinaryReader<R> {
    fn new(inner: R) -> Self {
        Self { inner, offset: 0 }
    }

    fn bytes(&mut self, length: usize) -> Result<Vec<u8>, String> {
        let mut bytes = vec![0; length];
        self.inner
            .read_exact(&mut bytes)
            .map_err(|error| format!("binary read failed at byte {}: {error}", self.offset))?;
        self.offset += length;
        Ok(bytes)
    }

    fn i32(&mut self) -> Result<i32, String> {
        let bytes: [u8; 4] = self
            .bytes(4)?
            .try_into()
            .expect("requested exactly four bytes");
        Ok(i32::from_le_bytes(bytes))
    }

    fn count(&mut self, label: &str) -> Result<usize, String> {
        let value = self.i32()?;
        usize::try_from(value)
            .map_err(|_| format!("negative {label} {value} at byte {}", self.offset - 4))
    }

    fn string(&mut self) -> Result<String, String> {
        let mut length = 0usize;
        let mut shift = 0u32;
        loop {
            if shift >= usize::BITS {
                return Err(format!(
                    "invalid 7-bit string length at byte {}",
                    self.offset
                ));
            }
            let byte = self.bytes(1)?[0];
            length |= usize::from(byte & 0x7f) << shift;
            if byte & 0x80 == 0 {
                break;
            }
            shift += 7;
        }
        String::from_utf8(self.bytes(length)?).map_err(|error| {
            format!(
                "invalid UTF-8 string ending at byte {}: {error}",
                self.offset
            )
        })
    }

    fn coord(&mut self) -> Result<Coord, String> {
        Ok(Coord::new(self.i32()?, self.i32()?, self.i32()?))
    }

    fn direction(&mut self) -> Result<Direction, String> {
        Direction::from_i32(self.i32()?)
    }

    fn expect_eof(&mut self) -> Result<(), String> {
        let mut byte = [0];
        match self.inner.read(&mut byte) {
            Ok(0) => Ok(()),
            Ok(_) => Err(format!("trailing campaign data at byte {}", self.offset)),
            Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => Ok(()),
            Err(error) => Err(format!("could not check campaign EOF: {error}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn campaign_path() -> std::path::PathBuf {
        crate::data_root().join("campaign").join("merged_binary.gz")
    }

    #[test]
    fn loads_the_complete_owned_campaign() {
        let campaign = Campaign::load_gzip(campaign_path()).expect("campaign should parse");
        assert_eq!(campaign.island_names.len(), 205);
        assert_eq!(campaign.island_state_sources.len(), 205);
        assert_eq!(campaign.temples.len(), 30);
        assert_eq!(campaign.puzzle_ids().len(), 86);
        assert_eq!(campaign.player_positions.len(), 86);
        assert_eq!(campaign.puzzle_names().expect("display names").len(), 86);
        let names = campaign.puzzle_names().expect("display names");
        assert!(names.iter().any(|name| name == "Happy Pool"));
        assert!(names.iter().any(|name| name == "God Pillar"));
    }

    #[test]
    fn parses_every_original_state_string() {
        let campaign = Campaign::load_gzip(campaign_path()).expect("campaign should parse");
        campaign.merged_state().expect("merged state should parse");
        for (name, source) in &campaign.island_state_sources {
            GameState::parse(source)
                .unwrap_or_else(|error| panic!("island {name:?} did not parse: {error}"));
        }
    }
}
