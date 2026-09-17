use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use serde::ser::SerializeStruct;
use serde::{Deserialize, Serialize, Serializer};
use serde_json::Value;

pub const CAMPAIGN_SCHEMA: &str = "overcooked-campaign-v1";
pub const LEVEL_SCHEMA: &str = "overcooked-level-v1";

#[derive(Clone, Debug, Deserialize)]
pub struct Campaign {
    pub schema: String,
    pub source: Source,
    pub scoring: Scoring,
    pub main_campaign: Vec<CampaignLevel>,
    pub orders: Vec<OrderNode>,
    pub cooking_steps: Vec<CookingStep>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Source {
    pub title: String,
    pub steam_app_id: String,
    pub depot_manifest: String,
    pub unity_version: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Scoring {
    pub default_delivery_points: i64,
    pub expired_order_penalty: i64,
    pub single_player_chops_per_stage: u32,
    pub chop_impact_seconds: f64,
    pub tip_boundaries: Vec<TipBoundary>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct TipBoundary {
    pub remaining_fraction_exclusive_min: f64,
    pub points: i64,
}

#[derive(Clone, Debug, Deserialize)]
pub struct CampaignLevel {
    pub number: u8,
    pub directory_index: u8,
    pub label: String,
    pub star_cost: u16,
    pub variants: Vec<LevelVariant>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct LevelVariant {
    pub players: i8,
    pub scene: String,
    pub score_star_boundaries: [i64; 3],
    pub config: LevelConfig,
}

#[derive(Clone, Debug, Deserialize)]
pub struct LevelConfig {
    pub id: String,
    pub kind: String,
    pub plate_return_seconds: f64,
    #[serde(default)]
    pub order_lifetime_seconds: Option<f64>,
    #[serde(default)]
    pub seconds_between_orders: Option<f64>,
    #[serde(default)]
    pub duration_seconds: Option<f64>,
    #[serde(default)]
    pub rounds: Vec<RoundConfig>,
    #[serde(default)]
    pub manual_order: Vec<RecipeEntry>,
    #[serde(default)]
    pub phases: Vec<Vec<RecipeEntry>>,
    #[serde(default)]
    pub fire: Option<FireConfig>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct FireConfig {
    pub recovery_seconds: f64,
    pub flammability_seconds: f64,
    pub cooldown_seconds: f64,
    pub encouragement_suppressed_seconds: f64,
    pub cooldown_suppressed_seconds: f64,
}

impl LevelConfig {
    pub fn duration_ms(&self) -> u64 {
        seconds_to_ms(
            self.rounds
                .first()
                .map(|round| round.duration_seconds)
                .or(self.duration_seconds)
                .expect("validated duration"),
        )
    }

    pub fn recipe_entries(&self) -> Vec<RecipeEntry> {
        if let Some(round) = self.rounds.first() {
            return round
                .recipes
                .as_ref()
                .map(|recipes| recipes.entries.clone())
                .unwrap_or_default();
        }
        if !self.manual_order.is_empty() {
            return self.manual_order.clone();
        }
        self.phases.iter().flatten().cloned().collect()
    }

    pub fn scripted_entries(&self) -> Vec<RecipeEntry> {
        if let Some(round) = self.rounds.first()
            && !round.manual_order.is_empty()
        {
            return round.manual_order.clone();
        }
        if !self.manual_order.is_empty() {
            return self.manual_order.clone();
        }
        self.phases.iter().flatten().cloned().collect()
    }
}

#[derive(Clone, Debug, Deserialize)]
pub struct RoundConfig {
    pub duration_seconds: f64,
    pub recipes: Option<RecipeList>,
    #[serde(default)]
    pub manual_order: Vec<RecipeEntry>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct RecipeList {
    pub id: String,
    pub entries: Vec<RecipeEntry>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct RecipeEntry {
    pub order: Option<String>,
    pub weight: f64,
    pub base_points_multiplier: i64,
    pub additional_points: i64,
}

#[derive(Clone, Debug, Deserialize)]
pub struct OrderNode {
    pub id: String,
    pub kind: String,
    #[serde(default)]
    pub required: Vec<String>,
    #[serde(default)]
    pub optional: Vec<String>,
    #[serde(default)]
    pub cooking_step: Option<String>,
    #[serde(default)]
    pub cooked: bool,
}

#[derive(Clone, Debug, Deserialize)]
pub struct CookingStep {
    pub id: String,
    pub uid: i64,
}

#[derive(Clone, Debug, Deserialize)]
pub struct LevelLayout {
    pub schema: String,
    pub scene: String,
    pub build_index: u8,
    pub order_capacity: usize,
    #[serde(default)]
    pub boss_flow: Option<BossFlow>,
    pub grid_managers: Vec<GridManager>,
    pub walkable: Vec<GridCell>,
    #[serde(default)]
    pub fall_edges: Vec<FallEdge>,
    pub objects: Vec<GridObject>,
    pub players: Vec<PlayerSpawn>,
    pub initial_plates: Vec<InitialPlate>,
    pub cooking_utensils: Vec<CookingUtensil>,
    #[serde(default)]
    pub fire_extinguishers: Vec<FireExtinguisher>,
    pub systems: Vec<LevelSystem>,
    #[serde(default)]
    pub motions: Vec<Motion>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct BossFlow {
    pub platforms: Vec<String>,
    pub intermission_seconds: f64,
}

#[derive(Clone, Debug, Deserialize)]
pub struct FallEdge {
    pub grid_manager: String,
    pub from: Coordinate,
    pub dx: i32,
    pub dz: i32,
}

#[derive(Clone, Debug, Deserialize)]
pub struct GridManager {
    pub id: String,
    pub name: String,
    pub half_size: Point3,
    pub origin: Vector3,
    pub size: Vector3,
    pub world: Vector3,
    pub motion: Option<String>,
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[allow(non_snake_case)]
pub struct Point3 {
    pub X: i32,
    pub Y: i32,
    pub Z: i32,
}

#[derive(Clone, Copy, Debug, Default, Deserialize)]
pub struct Vector3 {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

impl Serialize for Vector3 {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let quantize = |value: f64| (value * 1_000_000.0).round() / 1_000_000.0;
        let mut state = serializer.serialize_struct("Vector3", 3)?;
        state.serialize_field("x", &quantize(self.x))?;
        state.serialize_field("y", &quantize(self.y))?;
        state.serialize_field("z", &quantize(self.z))?;
        state.end()
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct GridCell {
    pub grid_manager: String,
    pub x: i32,
    pub y: i32,
    pub z: i32,
    pub world: Vector3,
    #[serde(default)]
    pub motion: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct GridObject {
    pub id: String,
    pub name: String,
    pub layer: u8,
    pub grid_manager: String,
    pub grid: Coordinate,
    pub world: Vector3,
    pub dynamic: bool,
    #[serde(default)]
    pub motion: Option<String>,
    #[serde(default)]
    pub motion_transform: Option<String>,
    #[serde(default)]
    pub motion_local_position: Option<Vector3>,
    pub components: Vec<String>,
    pub features: HashMap<String, Value>,
}

impl GridObject {
    pub fn has(&self, component: &str) -> bool {
        self.components
            .iter()
            .any(|candidate| candidate == component)
    }

    pub fn feature(&self, component: &str) -> Option<&serde_json::Map<String, Value>> {
        self.features.get(component)?.as_object()
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
pub struct Coordinate {
    pub x: i32,
    pub y: i32,
    pub z: i32,
}

#[derive(Clone, Debug, Deserialize)]
pub struct PlayerSpawn {
    pub id: u8,
    pub name: String,
    pub grid_manager: String,
    pub grid: Coordinate,
    pub world: Vector3,
    pub respawn_seconds: f64,
    pub spawn_effect_seconds: f64,
}

#[derive(Clone, Debug, Deserialize)]
pub struct CookingUtensil {
    pub id: String,
    pub name: String,
    pub grid_manager: String,
    pub grid: Coordinate,
    pub world: Vector3,
    pub cooking_seconds: f64,
    pub cooking_step: String,
    pub container_capacity: usize,
    pub station_type: i64,
}

#[derive(Clone, Debug, Deserialize)]
pub struct FireExtinguisher {
    pub id: String,
    pub name: String,
    pub grid_manager: String,
    pub grid: Coordinate,
    pub world: Vector3,
    pub extinguish_seconds: f64,
    pub spray_distance: f64,
}

#[derive(Clone, Debug, Deserialize)]
pub struct InitialPlate {
    pub id: String,
    pub name: String,
    pub grid_manager: String,
    pub grid: Coordinate,
    pub world: Vector3,
}

#[derive(Clone, Debug, Deserialize)]
pub struct LevelSystem {
    pub id: String,
    pub kind: String,
    pub name: String,
    pub enabled: bool,
    #[serde(default)]
    pub object: Option<String>,
    pub grid_manager: String,
    pub grid: Coordinate,
    pub world: Vector3,
    pub fields: Value,
    #[serde(default)]
    pub target_animator: Option<String>,
    #[serde(default)]
    pub target_object: Option<String>,
    #[serde(default)]
    pub target_grid: Option<Coordinate>,
    #[serde(default)]
    pub motion: Option<String>,
    #[serde(default)]
    pub target_world: Option<Vector3>,
    #[serde(default)]
    pub targets: Vec<SystemTarget>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct SystemTarget {
    pub grid: Coordinate,
    pub world: Vector3,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Motion {
    pub id: String,
    pub name: String,
    pub controller: String,
    pub transform: String,
    pub initial_local_position: Vector3,
    pub initial_local_rotation: Quaternion,
    pub initial_world_position: Vector3,
    pub initial_world_rotation: Quaternion,
    pub default_state: usize,
    pub parameters: Vec<MotionParameter>,
    pub default_values: Value,
    pub states: Vec<MotionState>,
    pub clips: Vec<MotionClip>,
}

#[derive(Clone, Copy, Debug, Deserialize)]
pub struct Quaternion {
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub w: f64,
}

#[derive(Clone, Debug, Deserialize)]
pub struct MotionParameter {
    pub id: u64,
    pub name: String,
    #[serde(rename = "type")]
    pub kind: u8,
}

#[derive(Clone, Debug, Deserialize)]
pub struct MotionState {
    pub index: usize,
    pub id: u64,
    pub name: String,
    pub clip: Option<String>,
    pub speed: f64,
    pub cycle_offset: f64,
    #[serde(rename = "loop")]
    pub loop_: bool,
    pub transitions: Vec<MotionTransition>,
    pub behaviors: Vec<MotionBehavior>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct MotionTransition {
    pub destination: usize,
    pub duration: f64,
    pub exit_time: f64,
    pub has_exit_time: bool,
    pub conditions: Vec<MotionCondition>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct MotionCondition {
    pub mode: u8,
    pub parameter: String,
    pub threshold: f64,
}

#[derive(Clone, Debug, Deserialize)]
pub struct MotionBehavior {
    pub kind: String,
    pub fields: Value,
    #[serde(default)]
    pub target_animator: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct MotionClip {
    pub id: String,
    pub name: String,
    pub duration_seconds: f64,
    #[serde(rename = "loop")]
    pub loop_: bool,
    pub channels: HashMap<String, Vec<Option<MotionTrack>>>,
    #[serde(default)]
    pub transform_channels: HashMap<String, HashMap<String, Vec<Option<MotionTrack>>>>,
    #[serde(default)]
    pub properties: HashMap<String, Option<MotionTrack>>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MotionTrack {
    Curve {
        keys: Vec<MotionKey>,
    },
    Dense {
        begin: f64,
        sample_rate: f64,
        samples: Vec<f64>,
    },
    Constant {
        value: f64,
    },
}

#[derive(Clone, Debug, Deserialize)]
pub struct MotionKey {
    pub time: f64,
    pub coefficients: [f64; 4],
}

pub struct GameData {
    pub campaign: Campaign,
    pub level: CampaignLevel,
    pub variant: LevelVariant,
    pub layout: LevelLayout,
}

impl GameData {
    pub fn load(root: impl AsRef<Path>, level_number: u8) -> Result<Self, String> {
        let root = root.as_ref();
        let campaign: Campaign = read_json(root.join("campaign.json"))?;
        campaign.validate()?;
        let level = campaign
            .main_campaign
            .iter()
            .find(|level| level.number == level_number)
            .cloned()
            .ok_or_else(|| format!("unknown main campaign level {level_number}"))?;
        let variant = level
            .variants
            .iter()
            .find(|variant| variant.players == 1)
            .cloned()
            .ok_or_else(|| format!("level {level_number} has no single-player variant"))?;
        let layout: LevelLayout =
            read_json(root.join("levels").join(format!("{}.json", variant.scene)))?;
        layout.validate()?;
        if layout.scene != variant.scene {
            return Err(format!(
                "variant scene {:?} loaded layout {:?}",
                variant.scene, layout.scene
            ));
        }
        Ok(Self {
            campaign,
            level,
            variant,
            layout,
        })
    }
}

impl Campaign {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != CAMPAIGN_SCHEMA {
            return Err(format!("unknown campaign schema {:?}", self.schema));
        }
        if self.main_campaign.len() != 30 {
            return Err(format!(
                "expected 30 main campaign levels, found {}",
                self.main_campaign.len()
            ));
        }
        let orders = self
            .orders
            .iter()
            .map(|order| order.id.as_str())
            .collect::<HashSet<_>>();
        if orders.len() != self.orders.len() {
            return Err("duplicate order node id".to_owned());
        }
        for (index, level) in self.main_campaign.iter().enumerate() {
            if usize::from(level.number) != index + 1 || level.variants.len() != 4 {
                return Err(format!(
                    "invalid level numbering/variants at {:?}",
                    level.label
                ));
            }
            for variant in &level.variants {
                if variant.config.duration_ms() == 0 {
                    return Err(format!("{} has zero duration", variant.config.id));
                }
                for entry in variant
                    .config
                    .recipe_entries()
                    .iter()
                    .chain(variant.config.scripted_entries().iter())
                {
                    if let Some(order) = &entry.order
                        && !orders.contains(order.as_str())
                    {
                        return Err(format!(
                            "{} references unknown order {order}",
                            variant.config.id
                        ));
                    }
                }
            }
        }
        Ok(())
    }
}

impl LevelLayout {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != LEVEL_SCHEMA || self.scene.is_empty() {
            return Err(format!("invalid level layout {:?}", self.scene));
        }
        let managers = self
            .grid_managers
            .iter()
            .map(|manager| manager.id.as_str())
            .collect::<HashSet<_>>();
        if managers.is_empty() || self.walkable.is_empty() || self.players.len() < 2 {
            return Err(format!("{} has incomplete spatial data", self.scene));
        }
        for manager in self
            .walkable
            .iter()
            .map(|cell| cell.grid_manager.as_str())
            .chain(
                self.objects
                    .iter()
                    .map(|object| object.grid_manager.as_str()),
            )
            .chain(
                self.players
                    .iter()
                    .map(|player| player.grid_manager.as_str()),
            )
        {
            if !managers.contains(manager) {
                return Err(format!(
                    "{} references unknown grid manager {manager}",
                    self.scene
                ));
            }
        }
        let object_ids = self
            .objects
            .iter()
            .map(|object| object.id.as_str())
            .collect::<HashSet<_>>();
        if object_ids.len() != self.objects.len() {
            return Err(format!("{} has duplicate grid object ids", self.scene));
        }
        Ok(())
    }
}

fn read_json<T: for<'de> Deserialize<'de>>(path: PathBuf) -> Result<T, String> {
    let bytes =
        fs::read(&path).map_err(|error| format!("could not read {}: {error}", path.display()))?;
    serde_json::from_slice(&bytes).map_err(|error| format!("invalid {}: {error}", path.display()))
}

pub fn seconds_to_ms(seconds: f64) -> u64 {
    (seconds * 1_000.0).round() as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_imported_main_campaign_levels_validate() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("data/overcooked-1");
        for level in 1..=30 {
            let game = GameData::load(&root, level).expect("imported level");
            assert_eq!(game.level.number, level);
            assert_eq!(game.variant.players, 1);
            assert!(game.layout.walkable.len() >= 12);
        }
    }
}
