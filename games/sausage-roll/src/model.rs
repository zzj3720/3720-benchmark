use std::collections::HashSet;
use std::fmt;
use std::ops::{Add, Sub};

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, Hash, PartialEq, Serialize)]
pub struct Coord {
    pub x: i32,
    pub y: i32,
    pub z: i32,
}

impl Coord {
    pub const ZERO: Self = Self { x: 0, y: 0, z: 0 };
    pub const INVALID: Self = Self {
        x: i32::MIN,
        y: i32::MIN,
        z: i32::MIN,
    };

    pub const fn new(x: i32, y: i32, z: i32) -> Self {
        Self { x, y, z }
    }

    pub const fn scale(self, factor: i32) -> Self {
        Self::new(self.x * factor, self.y * factor, self.z * factor)
    }
}

impl Add for Coord {
    type Output = Self;

    fn add(self, rhs: Self) -> Self::Output {
        Self::new(self.x + rhs.x, self.y + rhs.y, self.z + rhs.z)
    }
}

impl Sub for Coord {
    type Output = Self;

    fn sub(self, rhs: Self) -> Self::Output {
        Self::new(self.x - rhs.x, self.y - rhs.y, self.z - rhs.z)
    }
}

impl fmt::Display for Coord {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "({},{},{})", self.x, self.y, self.z)
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
#[repr(i32)]
pub enum Direction {
    North = 0,
    South = 1,
    West = 2,
    East = 3,
    NorthEast = 4,
    SouthWest = 5,
    NorthWest = 6,
    SouthEast = 7,
    None = 8,
    Down = 9,
    Up = 10,
}

impl Direction {
    pub fn from_i32(value: i32) -> Result<Self, String> {
        match value {
            0 => Ok(Self::North),
            1 => Ok(Self::South),
            2 => Ok(Self::West),
            3 => Ok(Self::East),
            4 => Ok(Self::NorthEast),
            5 => Ok(Self::SouthWest),
            6 => Ok(Self::NorthWest),
            7 => Ok(Self::SouthEast),
            8 => Ok(Self::None),
            9 => Ok(Self::Down),
            10 => Ok(Self::Up),
            _ => Err(format!("invalid direction value {value}")),
        }
    }

    pub fn parse(value: &str) -> Result<Self, String> {
        match value.trim().to_ascii_lowercase().as_str() {
            "north" | "n" | "up" | "u" | "w" => Ok(Self::North),
            "south" | "s" | "down" | "d" => Ok(Self::South),
            "west" | "left" | "l" | "a" => Ok(Self::West),
            "east" | "right" | "r" => Ok(Self::East),
            _ => Err(format!("unknown direction {value:?}")),
        }
    }

    pub const fn delta(self) -> Coord {
        match self {
            Self::North => Coord::new(0, -1, 0),
            Self::South => Coord::new(0, 1, 0),
            Self::West => Coord::new(-1, 0, 0),
            Self::East => Coord::new(1, 0, 0),
            Self::NorthEast => Coord::new(1, -1, 0),
            Self::SouthWest => Coord::new(-1, 1, 0),
            Self::NorthWest => Coord::new(-1, -1, 0),
            Self::SouthEast => Coord::new(1, 1, 0),
            Self::None => Coord::ZERO,
            Self::Down => Coord::new(0, 0, -1),
            Self::Up => Coord::new(0, 0, 1),
        }
    }

    pub const fn inverse(self) -> Self {
        match self {
            Self::North => Self::South,
            Self::South => Self::North,
            Self::West => Self::East,
            Self::East => Self::West,
            Self::NorthEast => Self::SouthWest,
            Self::SouthWest => Self::NorthEast,
            Self::NorthWest => Self::SouthEast,
            Self::SouthEast => Self::NorthWest,
            Self::None => Self::None,
            Self::Down => Self::Up,
            Self::Up => Self::Down,
        }
    }

    pub const fn is_cardinal(self) -> bool {
        matches!(self, Self::North | Self::South | Self::West | Self::East)
    }

    pub const fn is_horizontal(self) -> bool {
        (self as i32) <= Self::None as i32
    }

    pub const fn is_flat(self) -> bool {
        (self as i32) < Self::None as i32
    }

    pub const fn is_vertical(self) -> bool {
        (self as i32) > Self::None as i32
    }

    pub const fn is_diagonal(self) -> bool {
        matches!(
            self,
            Self::NorthEast | Self::SouthWest | Self::NorthWest | Self::SouthEast
        )
    }

    pub const fn is_orthogonal(self) -> bool {
        (self as i32) < Self::NorthEast as i32
    }

    pub const fn is_valid(self) -> bool {
        !matches!(self, Self::None)
    }

    pub fn parallel_to(self, other: Self) -> bool {
        self.is_valid() && other.is_valid() && (self == other || self.inverse() == other)
    }

    pub fn normal_to(self, other: Self) -> bool {
        self != Self::None && other != Self::None && !self.parallel_to(other)
    }

    pub const fn clockwise_90(self) -> Self {
        match self {
            Self::North => Self::East,
            Self::South => Self::West,
            Self::West => Self::North,
            Self::East => Self::South,
            Self::NorthEast => Self::SouthEast,
            Self::SouthWest => Self::NorthWest,
            Self::NorthWest => Self::NorthEast,
            Self::SouthEast => Self::SouthWest,
            value => value,
        }
    }

    pub const fn clockwise_45(self) -> Self {
        match self {
            Self::North => Self::NorthEast,
            Self::South => Self::SouthWest,
            Self::West => Self::SouthEast,
            Self::East => Self::NorthWest,
            Self::NorthEast => Self::West,
            Self::SouthWest => Self::East,
            Self::NorthWest => Self::North,
            Self::SouthEast => Self::South,
            value => value,
        }
    }

    pub const fn counterclockwise_45(self) -> Self {
        match self {
            Self::North => Self::NorthWest,
            Self::South => Self::SouthEast,
            Self::West => Self::NorthEast,
            Self::East => Self::SouthWest,
            Self::NorthEast => Self::North,
            Self::SouthWest => Self::South,
            Self::NorthWest => Self::East,
            Self::SouthEast => Self::West,
            value => value,
        }
    }

    pub const fn rotate_90(self, clockwise: bool) -> Self {
        if clockwise {
            self.clockwise_90()
        } else {
            self.clockwise_90().inverse()
        }
    }

    pub fn left_of(self, other: Self) -> bool {
        if self.is_orthogonal() && other.is_orthogonal() {
            self.clockwise_90() == other
        } else {
            Self::continue_rotation(self, other) == self.clockwise_90()
        }
    }

    pub fn rotation_between(from: Self, to: Self) -> Option<Self> {
        const TABLE: [[i32; 4]; 4] = [
            [-1, -1, 6, 4],
            [-1, -1, 5, 7],
            [6, 5, -1, -1],
            [4, 7, -1, -1],
        ];
        if !from.is_cardinal() || !to.is_cardinal() {
            return None;
        }
        Self::from_i32(TABLE[from as usize][to as usize]).ok()
    }

    pub fn continue_rotation(from: Self, to: Self) -> Self {
        const TABLE: [[i32; 8]; 8] = [
            [8, 8, 8, 8, 6, 8, 4, 8],
            [8, 8, 8, 8, 8, 7, 8, 5],
            [8, 8, 8, 8, 8, 6, 5, 8],
            [8, 8, 8, 8, 7, 8, 8, 4],
            [3, 8, 8, 0, 8, 8, 8, 8],
            [8, 2, 1, 8, 8, 8, 8, 8],
            [2, 8, 0, 8, 8, 8, 8, 8],
            [8, 3, 8, 1, 8, 8, 8, 8],
        ];
        let from_index = from as usize;
        let to_index = to as usize;
        if from_index >= 8 || to_index >= 8 {
            return Self::None;
        }
        Self::from_i32(TABLE[to_index][from_index]).expect("direction table is valid")
    }
}

impl Add<Direction> for Coord {
    type Output = Self;

    fn add(self, rhs: Direction) -> Self::Output {
        self + rhs.delta()
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
#[repr(i32)]
pub enum EntityType {
    Ground = 0,
    Bbq = 1,
    Player = 2,
    Sausage = 3,
    Ladder = 4,
    SpectralSausage = 5,
    Barrier = 6,
    Fork = 7,
    Island = 8,
}

impl EntityType {
    pub fn from_i32(value: i32) -> Result<Self, String> {
        match value {
            0 => Ok(Self::Ground),
            1 => Ok(Self::Bbq),
            2 => Ok(Self::Player),
            3 => Ok(Self::Sausage),
            4 => Ok(Self::Ladder),
            5 => Ok(Self::SpectralSausage),
            6 => Ok(Self::Barrier),
            7 => Ok(Self::Fork),
            8 => Ok(Self::Island),
            _ => Err(format!("invalid entity type {value}")),
        }
    }

    pub const fn is_static(self) -> bool {
        !matches!(
            self,
            Self::Player | Self::Sausage | Self::Barrier | Self::Fork | Self::Island
        )
    }

    pub const fn is_extended(self) -> bool {
        matches!(self, Self::Sausage | Self::Island)
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Entity {
    pub pos: Coord,
    pub entity_type: EntityType,
    pub id: i32,
    pub direction: Direction,
    pub data: String,
    pub stuck_to: i32,
    pub rotation: i32,
    pub cook_data: i32,
    pub turn_direction: Direction,
    pub tile_number: i32,
    pub tile_set: i32,
    pub pivot: i32,
}

impl Entity {
    pub fn parse(value: &str) -> Result<Self, String> {
        let fields: Vec<&str> = value.split(',').collect();
        if fields.len() < 15 {
            return Err(format!(
                "entity requires at least 15 comma-separated fields, got {}",
                fields.len()
            ));
        }
        let int = |index: usize, name: &str| {
            fields[index]
                .parse::<i32>()
                .map_err(|error| format!("invalid entity {name}: {error}"))
        };
        Ok(Self {
            pos: Coord::new(int(0, "x")?, int(1, "y")?, int(2, "z")?),
            entity_type: EntityType::from_i32(int(3, "type")?)?,
            id: int(4, "id")?,
            direction: Direction::from_i32(int(5, "direction")?)?,
            data: if fields[6].is_empty() && int(3, "type")? == EntityType::Ground as i32 {
                "0".to_owned()
            } else {
                fields[6].to_owned()
            },
            stuck_to: int(7, "stuck_to")?,
            rotation: i32::from(int(8, "rotation")? == 1 || int(9, "legacy_rotation")? == 1),
            cook_data: int(10, "cook_data")?,
            turn_direction: Direction::from_i32(int(11, "turn_direction")?)?,
            tile_number: int(12, "tile_number")?,
            tile_set: int(13, "tile_set")?,
            pivot: int(14, "pivot")?,
        })
    }

    pub fn footprint(&self, player_has_fork: bool) -> Vec<Coord> {
        let extended = self.entity_type.is_extended()
            || (self.entity_type == EntityType::Player && player_has_fork);
        if extended {
            vec![self.pos, self.pos + self.direction]
        } else {
            vec![self.pos]
        }
    }

    pub fn lower_footprint(&self, player_has_fork: bool) -> Vec<Coord> {
        self.footprint(player_has_fork)
            .into_iter()
            .map(|pos| pos + Direction::Down)
            .collect()
    }

    pub fn border(&self, movement: Direction, player_has_fork: bool) -> Vec<Coord> {
        let footprint = self.footprint(player_has_fork);
        footprint
            .iter()
            .copied()
            .map(|pos| pos + movement)
            .filter(|target| !footprint.contains(target))
            .collect()
    }

    /// Reverses which endpoint is the anchor without changing the occupied
    /// cells. The original engine uses this canonicalization before a sausage
    /// balanced on a player/fork is turned as a "hat".
    pub fn pivot_in_place(&mut self) {
        if self.entity_type != EntityType::Sausage {
            return;
        }
        self.pos = self.pos + self.direction;
        self.direction = self.direction.inverse();
        self.pivot = 1 - self.pivot;
        let fields = self.data.split(';').collect::<Vec<_>>();
        if fields.len() == 3 {
            self.data = format!("{};{};{}", fields[0], fields[2], fields[1]);
        }
        let faces = [
            self.cook_data % 4,
            self.cook_data / 4 % 4,
            self.cook_data / 16 % 4,
            self.cook_data / 64 % 4,
        ];
        self.cook_data = faces[3] + 4 * faces[2] + 16 * faces[1] + 64 * faces[0];
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct GameState {
    pub overworld: bool,
    pub push_target_level: String,
    pub entities: Vec<Entity>,
    pub level_completed: Vec<String>,
    pub world_sausages_issued: Vec<String>,
    pub tile_set: i32,
    pub display_name: String,
    pub sausages_cooked: i32,
    pub music_seed: i32,
}

impl GameState {
    pub fn parse(value: &str) -> Result<Self, String> {
        let sections: Vec<&str> = value.split('*').collect();
        let mut records: Vec<&str> = sections
            .first()
            .copied()
            .unwrap_or_default()
            .split('|')
            .filter(|record| !record.is_empty())
            .collect();
        let mut overworld = true;
        let mut push_target_level = String::new();
        if let Some(prefix) = records.first().copied() {
            if let Some(level) = prefix.strip_prefix('I') {
                overworld = false;
                push_target_level = level.to_owned();
                records.remove(0);
            } else if prefix.starts_with('F') {
                records.remove(0);
            }
        }
        let entities = records
            .into_iter()
            .map(Entity::parse)
            .collect::<Result<Vec<_>, _>>()?;
        let list = |index: usize| {
            sections
                .get(index)
                .copied()
                .unwrap_or_default()
                .split(',')
                .filter(|item| !item.is_empty())
                .map(str::to_owned)
                .collect()
        };
        let number = |index: usize| {
            sections
                .get(index)
                .filter(|value| !value.is_empty())
                .unwrap_or(&"0")
                .parse::<i32>()
                .map_err(|error| format!("invalid state metadata field {index}: {error}"))
        };
        Ok(Self {
            overworld,
            push_target_level,
            entities,
            level_completed: list(1),
            world_sausages_issued: list(2),
            tile_set: number(3)?,
            display_name: sections.get(4).copied().unwrap_or_default().to_owned(),
            sausages_cooked: number(5)?,
            music_seed: number(6)?,
        })
    }

    pub fn player(&self) -> Option<&Entity> {
        self.entities
            .iter()
            .find(|entity| entity.entity_type == EntityType::Player)
    }

    pub fn fork(&self) -> Option<&Entity> {
        self.entities
            .iter()
            .find(|entity| entity.entity_type == EntityType::Fork)
    }

    pub fn sausages(&self) -> impl Iterator<Item = &Entity> {
        self.entities
            .iter()
            .filter(|entity| entity.entity_type == EntityType::Sausage)
    }

    pub fn occupied_cells(&self) -> HashSet<Coord> {
        let player_has_fork = self.fork().is_none();
        self.entities
            .iter()
            .filter(|entity| entity.entity_type != EntityType::Island)
            .flat_map(|entity| entity.footprint(player_has_fork))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_original_entity_layout() {
        let entity = Entity::parse("4,3,9,4,0,1,,-1,0,0,0,8,0,0,0,").expect("valid entity");
        assert_eq!(entity.pos, Coord::new(4, 3, 9));
        assert_eq!(entity.entity_type, EntityType::Ladder);
        assert_eq!(entity.direction, Direction::South);
    }

    #[test]
    fn direction_values_match_the_original_binary() {
        assert_eq!(Coord::ZERO + Direction::North, Coord::new(0, -1, 0));
        assert_eq!(Direction::North.inverse(), Direction::South);
        assert!(Direction::East.parallel_to(Direction::West));
    }

    #[test]
    fn direction_rotation_tables_cover_turning_intermediates() {
        assert_eq!(Direction::North.clockwise_90(), Direction::East);
        assert_eq!(Direction::West.clockwise_45(), Direction::SouthEast);
        assert_eq!(Direction::SouthEast.counterclockwise_45(), Direction::West);
        assert_eq!(
            Direction::rotation_between(Direction::North, Direction::East),
            Some(Direction::NorthEast)
        );
        assert_eq!(
            Direction::continue_rotation(Direction::North, Direction::NorthEast),
            Direction::East
        );
        assert!(Direction::North.left_of(Direction::East));
        assert!(!Direction::North.left_of(Direction::West));
    }

    #[test]
    fn sausage_pivot_preserves_cells_and_reverses_endpoint_data() {
        let mut entity =
            Entity::parse("3,4,0,3,42,3,M;left;right,-1,0,0,228,8,0,0,0,").expect("valid sausage");
        let before = entity.footprint(true).into_iter().collect::<HashSet<_>>();
        entity.pivot_in_place();
        assert_eq!(
            entity.footprint(true).into_iter().collect::<HashSet<_>>(),
            before
        );
        assert_eq!(entity.pos, Coord::new(4, 4, 0));
        assert_eq!(entity.direction, Direction::West);
        assert_eq!(entity.data, "M;right;left");
        assert_eq!(entity.pivot, 1);
    }
}
