use std::collections::{HashMap, HashSet, VecDeque};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::campaign::{
    Coordinate, GameData, GridCell, GridObject, Motion, MotionBehavior, MotionState, MotionTrack,
    Quaternion, RecipeEntry, Vector3, seconds_to_ms,
};

pub const STATE_SCHEMA: &str = "overcooked-state-v1";
const INTERACTION_DISTANCE: f64 = 2.05;
const MAX_HELD_INPUT_TIME_SCALE: u32 = 4;
const TRAVEL_MS_PER_CELL: u64 = 1_000;

#[derive(Clone, Copy, Debug)]
pub struct SessionConfig {
    pub time_scale: u32,
    pub seed: u64,
}

impl Default for SessionConfig {
    fn default() -> Self {
        Self {
            time_scale: 5,
            seed: 448_510,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    North,
    South,
    East,
    West,
}

impl Direction {
    fn vector(self) -> (f64, f64) {
        match self {
            Self::North => (0.0, 1.0),
            Self::South => (0.0, -1.0),
            Self::East => (1.0, 0.0),
            Self::West => (-1.0, 0.0),
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct Snapshot {
    pub schema: &'static str,
    pub campaign: CampaignView,
    pub shift: ShiftView,
    pub active_chef: u8,
    pub chefs: Vec<ChefView>,
    pub destinations: Vec<DestinationView>,
    pub orders: Vec<OrderView>,
    pub map: MapView,
    pub works: Vec<WorkView>,
    pub hazards: Vec<HazardView>,
    pub alarms: Vec<Alarm>,
    pub recent_events: Vec<Event>,
    pub controls: Vec<&'static str>,
}

#[derive(Clone, Debug, Serialize)]
pub struct CampaignView {
    pub source: String,
    pub level: u8,
    pub scene: String,
    pub score: i64,
    pub stars: u8,
    pub star_boundaries: [i64; 3],
}

#[derive(Clone, Debug, Serialize)]
pub struct ShiftView {
    pub status: &'static str,
    pub elapsed_ms: u64,
    pub duration_ms: u64,
    pub remaining_ms: u64,
}

#[derive(Clone, Debug, Serialize)]
pub struct ChefView {
    pub id: u8,
    pub active: bool,
    pub grid_manager: String,
    pub position: Coordinate,
    pub world: Vector3,
    pub facing: Direction,
    pub held: Option<Item>,
    pub respawning_ms: Option<u64>,
    pub travel: Option<TravelView>,
}

#[derive(Clone, Debug, Serialize)]
pub struct TravelView {
    pub target: String,
    pub due_ms: u64,
    pub remaining_ms: u64,
}

#[derive(Clone, Debug, Serialize)]
pub struct DestinationView {
    pub target: String,
    pub name: String,
    pub kind: &'static str,
    pub position: Coordinate,
    pub stand_position: Coordinate,
    pub steps: usize,
    pub travel_ms: u64,
    pub arrival_ms: u64,
}

#[derive(Clone, Debug, Serialize)]
pub struct OrderView {
    pub id: String,
    pub recipe: String,
    pub cooking_step: Option<String>,
    pub requirements: Vec<RecipeRequirementView>,
    pub opened_ms: u64,
    pub deadline_ms: u64,
    pub remaining_ms: u64,
}

#[derive(Clone, Debug, Serialize)]
pub struct RecipeRequirementView {
    pub id: String,
    pub quantity: u32,
    pub kind: String,
    pub required: Vec<String>,
    pub cooking_step: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct MapView {
    pub walkable: Vec<CellView>,
    pub objects: Vec<ObjectView>,
    pub systems: Vec<SystemView>,
}

#[derive(Clone, Debug, Serialize)]
pub struct CellView {
    pub grid_manager: String,
    pub position: Coordinate,
    pub world: Vector3,
    pub moving: bool,
    pub blocked: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct ObjectView {
    pub id: String,
    pub name: String,
    pub kind: &'static str,
    pub grid_manager: String,
    pub position: Coordinate,
    pub world: Vector3,
    pub item: Option<Item>,
    pub supply: Option<String>,
    pub processes_to: Option<String>,
    pub plate_count: Option<usize>,
    pub dirty_plate_count: Option<usize>,
    pub enabled: Option<bool>,
    pub fire_strength: Option<f64>,
}

#[derive(Clone, Debug, Serialize)]
pub struct SystemView {
    pub id: String,
    pub kind: String,
    pub name: String,
    pub grid_manager: String,
    pub position: Coordinate,
    pub world: Vector3,
    pub target: Option<String>,
    pub target_world: Option<Vector3>,
    pub active: bool,
    pub progress: Option<f64>,
}

#[derive(Clone, Debug, Serialize)]
pub struct WorkView {
    pub chef: u8,
    pub target: String,
    pub kind: &'static str,
    pub progress_ms: u64,
    pub required_ms: u64,
}

#[derive(Clone, Debug, Serialize)]
pub struct HazardView {
    pub id: String,
    pub kind: &'static str,
    pub world: Vector3,
    pub from: Option<Vector3>,
    pub to: Option<Vector3>,
    pub due_ms: u64,
    pub remaining_ms: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Alarm {
    pub id: String,
    pub due_ms: u64,
    pub note: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct Event {
    pub elapsed_ms: u64,
    pub kind: &'static str,
    pub message: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct Item {
    pub id: String,
    pub name: String,
    #[serde(flatten)]
    body: ItemBody,
}

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum ItemBody {
    Food {
        order: Option<String>,
        process: Option<ProcessSpec>,
        work_progress_ms: u64,
    },
    Container {
        base_order: Option<String>,
        capacity: usize,
        contents: Vec<String>,
        cooking: Option<Cooking>,
    },
    Plate {
        contents: Vec<String>,
    },
    DirtyPlateStack {
        count: usize,
    },
    Extinguisher {
        extinguish_ms: u64,
        spray_distance: f64,
    },
}

#[derive(Clone, Debug, Serialize)]
struct ProcessSpec {
    result_name: String,
    result: FoodProperties,
    required_ms: u64,
}

#[derive(Clone, Debug, Default, Serialize)]
struct FoodProperties {
    order: Option<String>,
    container_capacity: Option<usize>,
    cooking: Option<CookingSpec>,
}

#[derive(Clone, Debug, Serialize)]
struct CookingSpec {
    step: String,
    station_type: i64,
    duration_ms: u64,
}

#[derive(Clone, Debug, Serialize)]
struct Cooking {
    step: String,
    station_type: i64,
    duration_ms: u64,
    progress_ms: u64,
}

#[derive(Clone)]
struct Chef {
    id: u8,
    cell: usize,
    spawn_cell: usize,
    facing: Direction,
    held: Option<Item>,
    respawn_due_ms: Option<u64>,
    travel: Option<Travel>,
}

#[derive(Clone)]
struct Travel {
    target: String,
    destination: usize,
    due_ms: u64,
}

struct ActiveOrder {
    id: String,
    entry: RecipeEntry,
    opened_ms: u64,
    deadline_ms: u64,
}

#[derive(Clone)]
enum Work {
    Chop { chef: usize, target: String },
    Wash { chef: usize, target: String },
    Extinguish { chef: usize, target: String },
}

impl Work {
    fn target(&self) -> &str {
        match self {
            Self::Chop { target, .. }
            | Self::Wash { target, .. }
            | Self::Extinguish { target, .. } => target,
        }
    }
}

struct PlateStack {
    clean: bool,
    count: usize,
}

struct Sink {
    count: usize,
    progress_ms: u64,
    clean_ms: u64,
    drying_station: String,
}

struct PendingPlate {
    due_ms: u64,
    station: String,
}

struct LooseItem {
    grid_manager: String,
    grid: Coordinate,
    world: Vector3,
    item: Item,
}

struct MeteorManagerRuntime {
    system: String,
    next_spawn_ms: u64,
}

struct MeteorWarning {
    id: String,
    world: Vector3,
    impact_ms: u64,
}

struct FireballSpawnerRuntime {
    system: String,
    period_ms: u64,
    offsets_ms: Vec<u64>,
    offset_index: usize,
    next_spawn_ms: u64,
}

struct Fireball {
    id: String,
    from: Vector3,
    to: Vector3,
    spawned_ms: u64,
    due_ms: u64,
}

struct FireState {
    strength: f64,
    recovery_suppressed_ms: u64,
}

#[derive(Clone)]
enum BossTransition {
    Intermission { due_ms: u64 },
    Raising { platform: String },
    Lowering { platform: String },
}

struct ConveyorTransfer {
    source: String,
    target: String,
    started_ms: u64,
    midpoint_ms: u64,
    due_ms: u64,
    crossed_midpoint: bool,
}

#[derive(Clone)]
enum MotionValue {
    Bool(bool),
    Int(i64),
    Float(f64),
}

#[derive(Clone)]
enum MotionAction {
    Trigger(String),
    SetValue(String, MotionValue),
}

struct ScheduledMotionAction {
    due_ms: u64,
    action: MotionAction,
}

struct MotionRuntime {
    state: usize,
    state_started_ms: u64,
    triggers: HashSet<String>,
    values: HashMap<String, MotionValue>,
    scheduled: Vec<ScheduledMotionAction>,
}

pub struct Session<'a> {
    data: &'a GameData,
    config: SessionConfig,
    started: bool,
    elapsed_ms: u64,
    score: i64,
    active_chef: usize,
    chefs: Vec<Chef>,
    slots: HashMap<String, Item>,
    loose_items: HashMap<String, LooseItem>,
    stacks: HashMap<String, PlateStack>,
    sinks: HashMap<String, Sink>,
    cooking_stations: Vec<(String, i64)>,
    works: Vec<Option<Work>>,
    pending_plates: Vec<PendingPlate>,
    orders: Vec<ActiveOrder>,
    next_order_ms: u64,
    issued_orders: usize,
    recipe_counts: Vec<u32>,
    boss_phase: usize,
    boss_phase_index: usize,
    boss_ready: bool,
    boss_complete: bool,
    boss_transition: Option<BossTransition>,
    rng: u64,
    next_item: u64,
    alarms: Vec<Alarm>,
    events: Vec<Event>,
    walkable_lookup: HashMap<(String, i32, i32, i32), usize>,
    motions: HashMap<String, MotionRuntime>,
    switch_enabled: HashMap<String, bool>,
    conveyor_transfers: Vec<ConveyorTransfer>,
    occupied_zones: HashSet<String>,
    hazard_rng: u64,
    next_hazard: u64,
    meteor_managers: Vec<MeteorManagerRuntime>,
    meteors: Vec<MeteorWarning>,
    fireball_spawners: Vec<FireballSpawnerRuntime>,
    fireballs: Vec<Fireball>,
    fires: HashMap<String, FireState>,
    fire_exposure_ms: HashMap<String, u64>,
}

impl<'a> Session<'a> {
    pub fn new(data: &'a GameData, config: SessionConfig) -> Result<Self, String> {
        if config.time_scale == 0 {
            return Err("time_scale must be at least one".to_owned());
        }
        let mut walkable_lookup = HashMap::new();
        for (index, cell) in data.layout.walkable.iter().enumerate() {
            if cell.motion.is_none() {
                walkable_lookup.insert((cell.grid_manager.clone(), cell.x, cell.y, cell.z), index);
            }
        }
        let chefs = data
            .layout
            .players
            .iter()
            .take(2)
            .map(|spawn| {
                let key = (
                    spawn.grid_manager.clone(),
                    spawn.grid.x,
                    spawn.grid.y,
                    spawn.grid.z,
                );
                let cell = walkable_lookup
                    .get(&key)
                    .copied()
                    .or_else(|| nearest_cell(&data.layout.walkable, spawn.world))
                    .ok_or_else(|| format!("chef {} has no walkable spawn", spawn.id))?;
                Ok(Chef {
                    id: spawn.id,
                    cell,
                    spawn_cell: cell,
                    facing: Direction::South,
                    held: None,
                    respawn_due_ms: None,
                    travel: None,
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        if chefs.len() != 2 {
            return Err("single-player requires the original two chef avatars".to_owned());
        }
        let cooking_stations = data
            .layout
            .objects
            .iter()
            .filter_map(|object| {
                object
                    .feature("CookingStation")?
                    .get("stationType")?
                    .as_i64()
                    .map(|station_type| (object.id.clone(), station_type))
            })
            .collect();

        let mut session = Self {
            data,
            config,
            started: false,
            elapsed_ms: 0,
            score: 0,
            active_chef: 0,
            chefs,
            slots: HashMap::new(),
            loose_items: HashMap::new(),
            stacks: HashMap::new(),
            sinks: HashMap::new(),
            cooking_stations,
            works: vec![None, None],
            pending_plates: Vec::new(),
            orders: Vec::new(),
            next_order_ms: 0,
            issued_orders: 0,
            recipe_counts: vec![0; data.variant.config.recipe_entries().len()],
            boss_phase: 0,
            boss_phase_index: 0,
            boss_ready: data.layout.boss_flow.is_none(),
            boss_complete: false,
            boss_transition: None,
            rng: config.seed,
            next_item: 1,
            alarms: Vec::new(),
            events: Vec::new(),
            walkable_lookup,
            motions: HashMap::new(),
            switch_enabled: HashMap::new(),
            conveyor_transfers: Vec::new(),
            occupied_zones: HashSet::new(),
            hazard_rng: config.seed ^ 0x9e37_79b9_7f4a_7c15,
            next_hazard: 1,
            meteor_managers: Vec::new(),
            meteors: Vec::new(),
            fireball_spawners: Vec::new(),
            fireballs: Vec::new(),
            fires: HashMap::new(),
            fire_exposure_ms: HashMap::new(),
        };
        session.initialize_switches();
        session.initialize_motions()?;
        session.initialize_stations()?;
        session.initialize_hazards()?;
        Ok(session)
    }

    pub const fn started(&self) -> bool {
        self.started
    }

    pub const fn elapsed_ms(&self) -> u64 {
        self.elapsed_ms
    }

    pub fn duration_ms(&self) -> u64 {
        self.scale(self.data.variant.config.duration_ms())
    }

    pub fn start(&mut self) -> Result<(), String> {
        if self.started {
            return Err("kitchen has already started".to_owned());
        }
        self.started = true;
        if let Some(platform) = self
            .data
            .layout
            .boss_flow
            .as_ref()
            .and_then(|flow| flow.platforms.first())
            .cloned()
        {
            self.set_motion_bool(&platform, "Down", true)?;
            self.boss_transition = Some(BossTransition::Lowering { platform });
        }
        self.process_due()?;
        self.event("shift_started", "The kitchen shift started.");
        Ok(())
    }

    pub fn advance_to(&mut self, target_ms: u64) -> Result<(), String> {
        if !self.started {
            if target_ms == 0 {
                return Ok(());
            }
            return Err("cannot advance the kitchen before start".to_owned());
        }
        if target_ms < self.elapsed_ms {
            return Err(format!(
                "clock cannot move backwards from {} to {target_ms}",
                self.elapsed_ms
            ));
        }
        let target_ms = target_ms.min(self.duration_ms());
        self.process_due()?;
        while self.elapsed_ms < target_ms && !self.boss_complete {
            let next = self.next_boundary(target_ms);
            let delta = next.saturating_sub(self.elapsed_ms);
            self.elapsed_ms = next;
            self.advance_continuous(delta);
            self.process_due()?;
        }
        Ok(())
    }

    pub fn switch(&mut self) -> Result<(), String> {
        self.require_running()?;
        self.active_chef = 1 - self.active_chef;
        self.event(
            "chef_switched",
            format!(
                "Control switched to chef {}.",
                self.chefs[self.active_chef].id
            ),
        );
        Ok(())
    }

    pub fn move_chef(&mut self, direction: Direction, dash: bool) -> Result<(), String> {
        self.require_running()?;
        self.require_active_chef()?;
        self.require_idle_travel()?;
        if self.works[self.active_chef].is_some() {
            return Err("stop the current work before moving".to_owned());
        }
        let steps = if dash { 2 } else { 1 };
        for _ in 0..steps {
            let current = self.chefs[self.active_chef].cell;
            let Some(next) = self.movement_neighbor(current, direction) else {
                if self.is_fall_edge(current, direction) {
                    self.chefs[self.active_chef].facing = direction;
                    self.kill_chef(self.active_chef, true)?;
                    self.refresh_trigger_zones()?;
                    return Ok(());
                }
                return Err("that direction is blocked".to_owned());
            };
            if self.chefs.iter().enumerate().any(|(index, chef)| {
                index != self.active_chef && chef.respawn_due_ms.is_none() && chef.cell == next
            }) {
                return Err("the other chef blocks that cell".to_owned());
            }
            self.chefs[self.active_chef].cell = next;
        }
        self.chefs[self.active_chef].facing = direction;
        self.refresh_trigger_zones()?;
        Ok(())
    }

    pub fn go(&mut self, target: &str) -> Result<u64, String> {
        self.require_running()?;
        self.require_active_chef()?;
        self.require_idle_travel()?;
        if self.works[self.active_chef].is_some() {
            return Err("stop the current work before travelling".to_owned());
        }
        let (destination, steps) = self
            .route_near(self.active_chef, target)?
            .ok_or_else(|| format!("target {target} is not currently reachable"))?;
        if steps == 0 {
            return Err(format!("chef is already at {target}"));
        }
        let due_ms = self
            .elapsed_ms
            .saturating_add((steps as u64).saturating_mul(TRAVEL_MS_PER_CELL));
        self.chefs[self.active_chef].travel = Some(Travel {
            target: target.to_owned(),
            destination,
            due_ms,
        });
        self.event(
            "travel_started",
            format!(
                "Chef {} started travelling to {target}; arrival is due at {due_ms} ms.",
                self.active_chef().id
            ),
        );
        Ok(due_ms)
    }

    pub fn interact(&mut self, target: &str) -> Result<(), String> {
        self.require_running()?;
        self.require_active_chef()?;
        self.require_idle_travel()?;
        if self.works[self.active_chef].is_some() {
            return Err("stop the current work before interacting".to_owned());
        }
        if let Some(loose) = self.loose_items.get(target) {
            if self.active_chef().held.is_some() {
                return Err("the active chef's hands are full".to_owned());
            }
            if distance(self.cell_world(self.active_chef().cell), loose.world)
                > INTERACTION_DISTANCE
            {
                return Err(format!("target {target} is out of reach"));
            }
            let item = self
                .loose_items
                .remove(target)
                .expect("checked loose item")
                .item;
            self.active_chef_mut().held = Some(item);
            return Ok(());
        }
        let object = self.object(target)?;
        self.require_near(object)?;
        if self.fires.contains_key(target) {
            return Err("that station is disabled by fire".to_owned());
        }
        if object.has("PlateStation") {
            return self.deliver(target);
        }
        if object.has("RubbishBin") {
            let held = self.active_chef_mut().held.take();
            return held
                .map(|_| ())
                .ok_or_else(|| "nothing is held to discard".to_owned());
        }
        if object.has("PickupItemSpawner") {
            return self.take_from_spawner(target);
        }
        if object.has("WashingStation") {
            return self.place_dirty_stack(target);
        }
        if object.has("PlateReturnStation") {
            return self.take_from_plate_stack(target);
        }
        if object.has("Interactable") && !object.has("AttachStation") {
            return self.use_switch(target);
        }
        self.interact_with_slot(target)
    }

    pub fn start_work(&mut self, target: &str) -> Result<u64, String> {
        self.require_running()?;
        self.require_active_chef()?;
        self.require_idle_travel()?;
        if self.works[self.active_chef].is_some() {
            return Err("the active chef is already holding a work input".to_owned());
        }
        if self
            .works
            .iter()
            .flatten()
            .any(|work| work.target() == target)
        {
            return Err("the other chef is already working at that target".to_owned());
        }
        let object = self.object(target)?;
        if let Some(Item {
            body:
                ItemBody::Extinguisher {
                    extinguish_ms,
                    spray_distance,
                },
            ..
        }) = self.active_chef().held.as_ref()
        {
            if !self.fires.contains_key(target) {
                return Err("the extinguisher must be aimed at an active fire".to_owned());
            }
            if distance(
                self.cell_world(self.active_chef().cell),
                self.object_world(object),
            ) > *spray_distance
            {
                return Err("the fire is outside the extinguisher spray".to_owned());
            }
            let due = self.elapsed_ms + *extinguish_ms;
            let suppression = self.data.variant.config.fire.as_ref().map_or(0, |config| {
                self.scale(seconds_to_ms(config.encouragement_suppressed_seconds))
            });
            if let Some(fire) = self.fires.get_mut(target) {
                fire.recovery_suppressed_ms = suppression;
            }
            self.works[self.active_chef] = Some(Work::Extinguish {
                chef: self.active_chef,
                target: target.to_owned(),
            });
            return Ok(due);
        }
        self.require_near(object)?;
        if self.fires.contains_key(target) {
            return Err("that station is disabled by fire".to_owned());
        }
        if object.has("Workstation") {
            let item = self
                .slots
                .get(target)
                .ok_or_else(|| "the workstation is empty".to_owned())?;
            let ItemBody::Food {
                process: Some(process),
                work_progress_ms,
                ..
            } = &item.body
            else {
                return Err("that item cannot be processed here".to_owned());
            };
            let remaining = process.required_ms.saturating_sub(*work_progress_ms);
            self.works[self.active_chef] = Some(Work::Chop {
                chef: self.active_chef,
                target: target.to_owned(),
            });
            return Ok(self.elapsed_ms + remaining);
        }
        if object.has("WashingStation") {
            let sink = self
                .sinks
                .get(target)
                .ok_or_else(|| "unknown washing station".to_owned())?;
            if sink.count == 0 {
                return Err("the sink has no dirty plates".to_owned());
            }
            let remaining = sink.clean_ms.saturating_sub(sink.progress_ms);
            self.works[self.active_chef] = Some(Work::Wash {
                chef: self.active_chef,
                target: target.to_owned(),
            });
            return Ok(self.elapsed_ms + remaining);
        }
        Err("target is not a workstation or sink".to_owned())
    }

    pub fn stop_work(&mut self) -> Result<(), String> {
        self.require_running()?;
        self.works[self.active_chef]
            .take()
            .map(|_| ())
            .ok_or_else(|| "the active chef is not holding a work input".to_owned())
    }

    pub fn set_alarm(&mut self, id: &str, after_ms: u64, note: &str) -> Result<u64, String> {
        self.require_running()?;
        if id.is_empty() || after_ms == 0 {
            return Err("alarm id and a positive delay are required".to_owned());
        }
        if self.alarms.iter().any(|alarm| alarm.id == id) {
            return Err(format!("alarm {id:?} already exists"));
        }
        let due_ms = self
            .elapsed_ms
            .saturating_add(after_ms)
            .min(self.duration_ms());
        self.alarms.push(Alarm {
            id: id.to_owned(),
            due_ms,
            note: note.to_owned(),
        });
        self.alarms.sort_by_key(|alarm| alarm.due_ms);
        Ok(due_ms)
    }

    pub fn cancel_alarm(&mut self, id: &str) -> Result<(), String> {
        let before = self.alarms.len();
        self.alarms.retain(|alarm| alarm.id != id);
        if self.alarms.len() == before {
            return Err(format!("unknown alarm {id:?}"));
        }
        Ok(())
    }

    pub fn deliver_due_alarms(&mut self) -> Vec<Alarm> {
        let mut due = Vec::new();
        self.alarms.retain(|alarm| {
            if alarm.due_ms <= self.elapsed_ms {
                due.push(alarm.clone());
                false
            } else {
                true
            }
        });
        due
    }

    pub fn next_pending_alarm_ms(&self) -> Option<u64> {
        self.alarms.first().map(|alarm| alarm.due_ms)
    }

    pub fn next_attention_ms(&self) -> Option<u64> {
        self.next_pending_alarm_ms()
            .into_iter()
            .chain(
                self.chefs
                    .iter()
                    .filter_map(|chef| chef.travel.as_ref().map(|travel| travel.due_ms)),
            )
            .min()
    }

    pub fn snapshot(&self) -> Snapshot {
        let mut loose_items = self.loose_items.iter().collect::<Vec<_>>();
        loose_items.sort_by_key(|(id, _)| id.as_str());
        let mut fires = self.fires.keys().collect::<Vec<_>>();
        fires.sort();
        Snapshot {
            schema: STATE_SCHEMA,
            campaign: CampaignView {
                source: self.data.campaign.source.title.clone(),
                level: self.data.level.number,
                scene: self.data.layout.scene.clone(),
                score: self.score,
                stars: self.stars(),
                star_boundaries: self.data.variant.score_star_boundaries,
            },
            shift: ShiftView {
                status: if !self.started {
                    "not_started"
                } else if self.boss_complete || self.elapsed_ms >= self.duration_ms() {
                    "complete"
                } else {
                    "running"
                },
                elapsed_ms: self.elapsed_ms,
                duration_ms: self.duration_ms(),
                remaining_ms: self.duration_ms().saturating_sub(self.elapsed_ms),
            },
            active_chef: self.chefs[self.active_chef].id,
            chefs: self
                .chefs
                .iter()
                .enumerate()
                .map(|(index, chef)| {
                    let cell = &self.data.layout.walkable[chef.cell];
                    ChefView {
                        id: chef.id,
                        active: index == self.active_chef,
                        grid_manager: cell.grid_manager.clone(),
                        position: cell_coordinate(cell),
                        world: self.cell_world(chef.cell),
                        facing: chef.facing,
                        held: chef.held.clone(),
                        respawning_ms: chef
                            .respawn_due_ms
                            .map(|due| due.saturating_sub(self.elapsed_ms)),
                        travel: chef.travel.as_ref().map(|travel| TravelView {
                            target: travel.target.clone(),
                            due_ms: travel.due_ms,
                            remaining_ms: travel.due_ms.saturating_sub(self.elapsed_ms),
                        }),
                    }
                })
                .collect(),
            destinations: self.destinations(),
            orders: self
                .orders
                .iter()
                .map(|order| OrderView {
                    id: order.id.clone(),
                    recipe: order.entry.order.clone().unwrap_or_default(),
                    cooking_step: order.entry.order.as_deref().and_then(|recipe| {
                        self.data
                            .campaign
                            .orders
                            .iter()
                            .find(|node| node.id == recipe)
                            .and_then(|node| node.cooking_step.clone())
                    }),
                    requirements: order
                        .entry
                        .order
                        .as_deref()
                        .map_or_else(Vec::new, |recipe| self.recipe_requirements(recipe)),
                    opened_ms: order.opened_ms,
                    deadline_ms: order.deadline_ms,
                    remaining_ms: order.deadline_ms.saturating_sub(self.elapsed_ms),
                })
                .collect(),
            map: MapView {
                walkable: self
                    .data
                    .layout
                    .walkable
                    .iter()
                    .enumerate()
                    .map(|(index, cell)| CellView {
                        grid_manager: cell.grid_manager.clone(),
                        position: cell_coordinate(cell),
                        world: self.cell_world(index),
                        moving: cell.motion.is_some(),
                        blocked: self.cell_blocked(index),
                    })
                    .collect(),
                objects: self
                    .data
                    .layout
                    .objects
                    .iter()
                    .map(|object| self.object_view(object))
                    .chain(loose_items.into_iter().map(|(id, loose)| ObjectView {
                        id: id.clone(),
                        name: loose.item.name.clone(),
                        kind: "loose_item",
                        grid_manager: loose.grid_manager.clone(),
                        position: loose.grid,
                        world: loose.world,
                        item: Some(loose.item.clone()),
                        supply: None,
                        processes_to: None,
                        plate_count: None,
                        dirty_plate_count: None,
                        enabled: None,
                        fire_strength: None,
                    }))
                    .collect(),
                systems: self
                    .data
                    .layout
                    .systems
                    .iter()
                    .map(|system| SystemView {
                        id: system.id.clone(),
                        kind: system.kind.clone(),
                        name: system.name.clone(),
                        grid_manager: system.grid_manager.clone(),
                        position: system.grid,
                        world: system.world,
                        target: system.target_object.clone(),
                        target_world: system.target_world,
                        active: self.occupied_zones.contains(&system.id)
                            || self.conveyor_transfers.iter().any(|transfer| {
                                system.object.as_deref() == Some(transfer.source.as_str())
                            }),
                        progress: self.conveyor_transfers.iter().find_map(|transfer| {
                            (system.object.as_deref() == Some(transfer.source.as_str())).then(
                                || {
                                    let duration =
                                        transfer.due_ms.saturating_sub(transfer.started_ms).max(1);
                                    self.elapsed_ms.saturating_sub(transfer.started_ms) as f64
                                        / duration as f64
                                },
                            )
                        }),
                    })
                    .collect(),
            },
            works: self.work_views(),
            hazards: self
                .meteors
                .iter()
                .map(|meteor| HazardView {
                    id: meteor.id.clone(),
                    kind: "meteor",
                    world: meteor.world,
                    from: None,
                    to: Some(meteor.world),
                    due_ms: meteor.impact_ms,
                    remaining_ms: meteor.impact_ms.saturating_sub(self.elapsed_ms),
                })
                .chain(self.fireball_spawners.iter().filter_map(|spawner| {
                    let system = self
                        .data
                        .layout
                        .systems
                        .iter()
                        .find(|system| system.id == spawner.system)?;
                    Some(HazardView {
                        id: format!("{}-next", spawner.system),
                        kind: "fireball_warning",
                        world: system.world,
                        from: Some(system.world),
                        to: system.target_world,
                        due_ms: spawner.next_spawn_ms,
                        remaining_ms: spawner.next_spawn_ms.saturating_sub(self.elapsed_ms),
                    })
                }))
                .chain(self.fireballs.iter().map(|fireball| HazardView {
                    id: fireball.id.clone(),
                    kind: "fireball",
                    world: fireball_position(fireball, self.elapsed_ms),
                    from: Some(fireball.from),
                    to: Some(fireball.to),
                    due_ms: fireball.due_ms,
                    remaining_ms: fireball.due_ms.saturating_sub(self.elapsed_ms),
                }))
                .chain(fires.into_iter().filter_map(|id| {
                    self.data
                        .layout
                        .objects
                        .iter()
                        .find(|object| object.id == *id)
                        .map(|object| HazardView {
                            id: id.clone(),
                            kind: "fire",
                            world: self.object_world(object),
                            from: None,
                            to: None,
                            due_ms: 0,
                            remaining_ms: 0,
                        })
                }))
                .collect(),
            alarms: self.alarms.clone(),
            recent_events: self.events.iter().rev().take(16).cloned().collect(),
            controls: vec![
                "show",
                "start",
                "go",
                "switch",
                "interact",
                "start_work",
                "stop_work",
                "alarm",
                "cancel",
                "wait",
                "submit",
            ],
        }
    }

    pub fn final_score(&mut self) -> i64 {
        if self.started && !self.boss_complete {
            let _ = self.advance_to(self.duration_ms());
        }
        self.score
    }

    fn initialize_motions(&mut self) -> Result<(), String> {
        for motion in &self.data.layout.motions {
            if motion.default_state >= motion.states.len() {
                return Err(format!(
                    "{} has invalid default state {}",
                    motion.id, motion.default_state
                ));
            }
            let defaults = motion
                .default_values
                .as_object()
                .ok_or_else(|| format!("{} has invalid default values", motion.id))?;
            let bools = defaults
                .get("m_BoolValues")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            let ints = defaults
                .get("m_IntValues")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            let floats = defaults
                .get("m_FloatValues")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            let (mut bool_index, mut int_index, mut float_index) = (0, 0, 0);
            let mut values = HashMap::new();
            for parameter in &motion.parameters {
                match parameter.kind {
                    4 | 9 => {
                        if parameter.kind == 4 {
                            values.insert(
                                parameter.name.clone(),
                                MotionValue::Bool(
                                    bools
                                        .get(bool_index)
                                        .and_then(Value::as_bool)
                                        .unwrap_or(false),
                                ),
                            );
                        }
                        bool_index += 1;
                    }
                    3 => {
                        values.insert(
                            parameter.name.clone(),
                            MotionValue::Int(
                                ints.get(int_index).and_then(Value::as_i64).unwrap_or(0),
                            ),
                        );
                        int_index += 1;
                    }
                    1 => {
                        values.insert(
                            parameter.name.clone(),
                            MotionValue::Float(
                                floats
                                    .get(float_index)
                                    .and_then(Value::as_f64)
                                    .unwrap_or(0.0),
                            ),
                        );
                        float_index += 1;
                    }
                    _ => {}
                }
            }
            self.motions.insert(
                motion.id.clone(),
                MotionRuntime {
                    state: motion.default_state,
                    state_started_ms: 0,
                    triggers: HashSet::new(),
                    values,
                    scheduled: Vec::new(),
                },
            );
        }
        let awake_values = self
            .data
            .layout
            .systems
            .iter()
            .filter(|system| system.kind == "TriggerAnimatorSetVariable")
            .filter(|system| system.fields.get("m_onAwake").and_then(Value::as_i64) == Some(1))
            .filter_map(|system| {
                Some((
                    system.target_animator.clone()?,
                    system.fields.get("m_variableName")?.as_str()?.to_owned(),
                    motion_value(&system.fields)?,
                ))
            })
            .collect::<Vec<_>>();
        for (motion, name, value) in awake_values {
            if let Some(runtime) = self.motions.get_mut(&motion) {
                runtime.values.insert(name, value);
            }
        }
        let initial = self
            .data
            .layout
            .motions
            .iter()
            .map(|motion| (motion.id.clone(), motion.default_state))
            .collect::<Vec<_>>();
        for (motion, state) in initial {
            self.enter_motion_state(&motion, state)?;
        }
        self.process_motion_due()
    }

    fn initialize_switches(&mut self) {
        for object in &self.data.layout.objects {
            if object.has("Interactable") && !object.has("AttachStation") {
                let enabled = self
                    .data
                    .layout
                    .systems
                    .iter()
                    .find(|system| {
                        system.kind == "TriggerDisableScript" && system.name == object.name
                    })
                    .and_then(|system| system.fields.get("m_startEnabled"))
                    .and_then(Value::as_i64)
                    .is_none_or(|value| value != 0);
                self.switch_enabled.insert(object.id.clone(), enabled);
            }
        }
    }

    fn use_switch(&mut self, target: &str) -> Result<(), String> {
        if !self.switch_enabled.get(target).copied().unwrap_or(true) {
            return Err("that switch is disabled while the mechanism moves".to_owned());
        }
        let object = self.object(target)?;
        let trigger = object
            .feature("Interactable")
            .and_then(|feature| feature.get("impulse_trigger"))
            .and_then(Value::as_str)
            .filter(|trigger| !trigger.is_empty())
            .ok_or_else(|| "that interactable has no impulse action".to_owned())?
            .to_owned();
        let name = object.name.clone();
        self.dispatch_object_trigger(&name, &trigger)?;
        self.event("switch_used", format!("{name} fired {trigger}."));
        Ok(())
    }

    fn dispatch_object_trigger(&mut self, name: &str, initial: &str) -> Result<(), String> {
        let systems = self
            .data
            .layout
            .systems
            .iter()
            .filter(|system| system.name == name)
            .cloned()
            .collect::<Vec<_>>();
        let mut pending = vec![initial.to_owned()];
        let mut seen = HashSet::new();
        while let Some(trigger) = pending.pop() {
            if !seen.insert(trigger.clone()) {
                continue;
            }
            for system in &systems {
                match system.kind.as_str() {
                    "TriggerAdapter"
                        if system.fields.get("m_inputTrigger").and_then(Value::as_str)
                            == Some(trigger.as_str()) =>
                    {
                        if let Some(output) =
                            system.fields.get("m_outputTrigger").and_then(Value::as_str)
                        {
                            pending.push(output.to_owned());
                        }
                    }
                    "TriggerDisableScript" => {
                        let enabled = system.fields.get("m_enableTrigger").and_then(Value::as_str)
                            == Some(trigger.as_str());
                        let disabled = system
                            .fields
                            .get("m_disableTrigger")
                            .and_then(Value::as_str)
                            == Some(trigger.as_str());
                        if enabled || disabled {
                            for object in &self.data.layout.objects {
                                if object.name == name
                                    && self.switch_enabled.contains_key(&object.id)
                                {
                                    self.switch_enabled.insert(object.id.clone(), enabled);
                                }
                            }
                        }
                    }
                    "TriggerOnAnimator"
                        if system
                            .fields
                            .get("m_triggerToReceive")
                            .and_then(Value::as_str)
                            == Some(trigger.as_str()) =>
                    {
                        if let (Some(target), Some(output)) = (
                            system.target_animator.as_deref(),
                            system.fields.get("m_triggerToFire").and_then(Value::as_str),
                        ) {
                            self.fire_motion_trigger(target, output)?;
                        }
                    }
                    "TriggerAnimatorSetVariable"
                        if system
                            .fields
                            .get("m_triggerToReceive")
                            .and_then(Value::as_str)
                            == Some(trigger.as_str()) =>
                    {
                        if let (Some(target), Some(variable), Some(value)) = (
                            system.target_animator.as_deref(),
                            system.fields.get("m_variableName").and_then(Value::as_str),
                            motion_value(&system.fields),
                        ) && let Some(runtime) = self.motions.get_mut(target)
                        {
                            runtime.values.insert(variable.to_owned(), value);
                        }
                    }
                    _ => {}
                }
            }
        }
        self.process_motion_due()
    }

    fn fire_motion_trigger(&mut self, motion: &str, trigger: &str) -> Result<(), String> {
        let runtime = self
            .motions
            .get_mut(motion)
            .ok_or_else(|| format!("trigger targets unknown gameplay animator {motion}"))?;
        runtime.triggers.insert(trigger.to_owned());
        Ok(())
    }

    fn set_motion_bool(&mut self, motion: &str, name: &str, value: bool) -> Result<(), String> {
        let runtime = self
            .motions
            .get_mut(motion)
            .ok_or_else(|| format!("variable targets unknown gameplay animator {motion}"))?;
        runtime
            .values
            .insert(name.to_owned(), MotionValue::Bool(value));
        self.process_motion_due()
    }

    fn enter_motion_state(&mut self, motion_id: &str, state_index: usize) -> Result<(), String> {
        let state = self
            .motion(motion_id)?
            .states
            .get(state_index)
            .cloned()
            .ok_or_else(|| format!("{motion_id} has unknown state {state_index}"))?;
        let runtime = self
            .motions
            .get_mut(motion_id)
            .ok_or_else(|| format!("missing runtime for {motion_id}"))?;
        runtime.state = state_index;
        runtime.state_started_ms = self.elapsed_ms;
        runtime.scheduled.clear();
        for behavior in state.behaviors {
            self.enter_motion_behavior(motion_id, behavior)?;
        }
        Ok(())
    }

    fn exit_motion_state(&mut self, motion_id: &str) -> Result<(), String> {
        let runtime = self
            .motions
            .get(motion_id)
            .ok_or_else(|| format!("missing runtime for {motion_id}"))?;
        let behaviors = self
            .motion(motion_id)?
            .states
            .get(runtime.state)
            .ok_or_else(|| format!("{motion_id} has invalid runtime state"))?
            .behaviors
            .clone();
        for behavior in behaviors {
            if behavior.kind != "SetBoolDuringState" {
                continue;
            }
            let fields = behavior
                .fields
                .as_object()
                .ok_or_else(|| "motion behavior fields must be an object".to_owned())?;
            let target = behavior.target_animator.as_deref().unwrap_or(motion_id);
            let variable = string(fields, "m_variableName")?;
            let invert = fields
                .get("m_invert")
                .and_then(Value::as_i64)
                .is_some_and(|value| value != 0);
            if let Some(runtime) = self.motions.get_mut(target) {
                runtime.values.insert(variable, MotionValue::Bool(invert));
            }
        }
        Ok(())
    }

    fn enter_motion_behavior(
        &mut self,
        current_motion: &str,
        behavior: MotionBehavior,
    ) -> Result<(), String> {
        let referenced_target = behavior.target_animator.clone();
        let fields = behavior
            .fields
            .as_object()
            .ok_or_else(|| "motion behavior fields must be an object".to_owned())?;
        match behavior.kind.as_str() {
            "SendTriggerAfterTime" => {
                let target = referenced_target.unwrap_or_else(|| current_motion.to_owned());
                let trigger = string(fields, "TriggerName")?;
                let seconds = number(fields, "TriggerTime")?;
                self.schedule_motion_trigger(&target, trigger, seconds);
            }
            "SendTriggerToObject" => {
                let object = string(fields, "m_objectName")?;
                let trigger = string(fields, "m_triggerToSend")?;
                self.dispatch_object_trigger(&object, &trigger)?;
            }
            "SendTriggerToAnotherAnimator" => {
                if let Some(target) = referenced_target {
                    let trigger = string(fields, "m_triggerName")?;
                    let seconds = number(fields, "m_triggerTime")?;
                    self.schedule_motion_trigger(&target, trigger, seconds);
                }
            }
            "SetBoolDuringState" => {
                let target = referenced_target.as_deref().unwrap_or(current_motion);
                let variable = string(fields, "m_variableName")?;
                let invert = fields
                    .get("m_invert")
                    .and_then(Value::as_i64)
                    .is_some_and(|value| value != 0);
                if let Some(runtime) = self.motions.get_mut(target) {
                    runtime.values.insert(variable, MotionValue::Bool(!invert));
                }
            }
            "SetVariableOnState" => {
                let target = referenced_target.as_deref().unwrap_or(current_motion);
                let seconds = number(fields, "m_triggerTime")?;
                for (field, value) in [
                    ("m_boolVariable", "bool"),
                    ("m_intVariable", "int"),
                    ("m_floatVariable", "float"),
                ] {
                    let Some(variable) = fields.get(field).and_then(Value::as_object) else {
                        continue;
                    };
                    let name = string(variable, "VariableName")?;
                    if name.is_empty() {
                        continue;
                    }
                    let value = match value {
                        "bool" => MotionValue::Bool(
                            variable
                                .get("Value")
                                .and_then(Value::as_i64)
                                .is_some_and(|value| value != 0),
                        ),
                        "int" => MotionValue::Int(
                            variable.get("Value").and_then(Value::as_i64).unwrap_or(0),
                        ),
                        _ => MotionValue::Float(
                            variable.get("Value").and_then(Value::as_f64).unwrap_or(0.0),
                        ),
                    };
                    self.schedule_motion_value(target, name, value, seconds);
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn schedule_motion_trigger(&mut self, motion: &str, trigger: String, seconds: f64) {
        self.schedule_motion_action(motion, MotionAction::Trigger(trigger), seconds);
    }

    fn schedule_motion_value(
        &mut self,
        motion: &str,
        name: String,
        value: MotionValue,
        seconds: f64,
    ) {
        self.schedule_motion_action(motion, MotionAction::SetValue(name, value), seconds);
    }

    fn schedule_motion_action(&mut self, motion: &str, action: MotionAction, seconds: f64) {
        let due_ms = self
            .elapsed_ms
            .saturating_add(self.scale(seconds_to_ms(seconds)));
        if let Some(runtime) = self.motions.get_mut(motion) {
            runtime
                .scheduled
                .push(ScheduledMotionAction { due_ms, action });
        }
    }

    fn process_motion_due(&mut self) -> Result<(), String> {
        for _ in 0..64 {
            let mut changed = false;
            let mut ids = self.motions.keys().cloned().collect::<Vec<_>>();
            ids.sort();
            for id in &ids {
                let due = {
                    let runtime = self.motions.get_mut(id).expect("known motion");
                    let mut due = Vec::new();
                    runtime.scheduled.retain(|scheduled| {
                        if scheduled.due_ms <= self.elapsed_ms {
                            due.push(scheduled.action.clone());
                            false
                        } else {
                            true
                        }
                    });
                    due
                };
                if !due.is_empty() {
                    let runtime = self.motions.get_mut(id).expect("known motion");
                    for action in due {
                        match action {
                            MotionAction::Trigger(trigger) => {
                                runtime.triggers.insert(trigger);
                            }
                            MotionAction::SetValue(name, value) => {
                                runtime.values.insert(name, value);
                            }
                        }
                    }
                    changed = true;
                }
            }
            for id in ids {
                if let Some(transition) = self.ready_motion_transition(&id)? {
                    if let Some(runtime) = self.motions.get_mut(&id) {
                        for condition in &transition.conditions {
                            runtime.triggers.remove(&condition.parameter);
                        }
                    }
                    self.exit_motion_state(&id)?;
                    self.enter_motion_state(&id, transition.destination)?;
                    changed = true;
                }
            }
            if !changed {
                return Ok(());
            }
        }
        Err("gameplay animator transition loop did not settle".to_owned())
    }

    fn ready_motion_transition(
        &self,
        motion_id: &str,
    ) -> Result<Option<crate::campaign::MotionTransition>, String> {
        let motion = self.motion(motion_id)?;
        let runtime = self
            .motions
            .get(motion_id)
            .ok_or_else(|| format!("missing runtime for {motion_id}"))?;
        let state = motion
            .states
            .get(runtime.state)
            .ok_or_else(|| format!("{motion_id} has invalid runtime state"))?;
        for transition in &state.transitions {
            if !transition
                .conditions
                .iter()
                .all(|condition| motion_condition(runtime, condition))
            {
                continue;
            }
            if transition.has_exit_time {
                let duration = self.motion_state_duration_ms(motion, state);
                let due = runtime.state_started_ms.saturating_add(
                    (duration as f64 * transition.exit_time.max(0.0)).round() as u64,
                );
                if self.elapsed_ms < due {
                    continue;
                }
            }
            if transition.conditions.is_empty() && !transition.has_exit_time {
                continue;
            }
            return Ok(Some(transition.clone()));
        }
        Ok(None)
    }

    fn motion_state_duration_ms(&self, motion: &Motion, state: &MotionState) -> u64 {
        let seconds = state
            .clip
            .as_ref()
            .and_then(|id| motion.clips.iter().find(|clip| clip.id == *id))
            .map_or(0.0, |clip| {
                clip.duration_seconds / state.speed.abs().max(0.000_001)
            });
        self.scale(seconds_to_ms(seconds))
    }

    fn motion(&self, id: &str) -> Result<&Motion, String> {
        self.data
            .layout
            .motions
            .iter()
            .find(|motion| motion.id == id)
            .ok_or_else(|| format!("unknown gameplay animator {id}"))
    }

    fn motion_local_pose(&self, id: &str) -> (Vector3, Quaternion) {
        let motion = self.motion(id).expect("validated gameplay animator");
        let runtime = self.motions.get(id).expect("initialized gameplay animator");
        let state = &motion.states[runtime.state];
        let Some(clip) = state
            .clip
            .as_ref()
            .and_then(|clip_id| motion.clips.iter().find(|clip| clip.id == *clip_id))
        else {
            return (motion.initial_local_position, motion.initial_local_rotation);
        };
        let time = self.motion_clip_time(runtime, state, clip);
        let mut position = motion.initial_local_position;
        if let Some(tracks) = clip.channels.get("position") {
            for (index, value) in [&mut position.x, &mut position.y, &mut position.z]
                .into_iter()
                .enumerate()
            {
                if let Some(track) = tracks.get(index).and_then(Option::as_ref) {
                    *value = motion_track_value(track, time);
                }
            }
        }
        let mut rotation = motion.initial_local_rotation;
        if let Some(tracks) = clip.channels.get("rotation") {
            for (index, value) in [
                &mut rotation.x,
                &mut rotation.y,
                &mut rotation.z,
                &mut rotation.w,
            ]
            .into_iter()
            .enumerate()
            {
                if let Some(track) = tracks.get(index).and_then(Option::as_ref) {
                    *value = motion_track_value(track, time);
                }
            }
            rotation = quaternion_normalize(rotation);
        } else if let Some(tracks) = clip.channels.get("euler") {
            let mut euler = [0.0; 3];
            for (index, value) in euler.iter_mut().enumerate() {
                if let Some(track) = tracks.get(index).and_then(Option::as_ref) {
                    *value = motion_track_value(track, time);
                }
            }
            rotation = quaternion_from_euler(euler);
        }
        (position, rotation)
    }

    fn motion_clip_time(
        &self,
        runtime: &MotionRuntime,
        state: &MotionState,
        clip: &crate::campaign::MotionClip,
    ) -> f64 {
        let duration = clip.duration_seconds.max(0.0);
        let mut time = self.elapsed_ms.saturating_sub(runtime.state_started_ms) as f64
            / f64::from(self.config.time_scale)
            / 1_000.0
            * state.speed.abs().max(0.000_001)
            + state.cycle_offset * duration;
        if duration > 0.0 {
            if state.loop_ || clip.loop_ {
                time %= duration;
            } else {
                time = time.min(duration);
            }
        }
        time
    }

    fn motion_root_world_pose(&self, motion_id: &str) -> (Vector3, Quaternion) {
        let motion = self.motion(motion_id).expect("validated gameplay animator");
        let (local_position, local_rotation) = self.motion_local_pose(motion_id);
        let parent_rotation = quaternion_multiply(
            motion.initial_world_rotation,
            quaternion_inverse(motion.initial_local_rotation),
        );
        let root_position = vector_add(
            motion.initial_world_position,
            quaternion_rotate_vector(
                parent_rotation,
                vector_sub(local_position, motion.initial_local_position),
            ),
        );
        let root_rotation = quaternion_multiply(parent_rotation, local_rotation);
        (root_position, root_rotation)
    }

    fn transform_world(&self, base: Vector3, motion_id: &str) -> Vector3 {
        let motion = self.motion(motion_id).expect("validated gameplay animator");
        let (root_position, root_rotation) = self.motion_root_world_pose(motion_id);
        let local_offset = quaternion_rotate_vector(
            quaternion_inverse(motion.initial_world_rotation),
            vector_sub(base, motion.initial_world_position),
        );
        vector_add(
            root_position,
            quaternion_rotate_vector(root_rotation, local_offset),
        )
    }

    fn cell_world(&self, index: usize) -> Vector3 {
        let cell = &self.data.layout.walkable[index];
        let motion = cell.motion.as_deref().or_else(|| {
            self.data
                .layout
                .grid_managers
                .iter()
                .find(|manager| manager.id == cell.grid_manager)
                .and_then(|manager| manager.motion.as_deref())
        });
        motion.map_or(cell.world, |id| self.transform_world(cell.world, id))
    }

    fn object_world(&self, object: &GridObject) -> Vector3 {
        let motion = object.motion.as_deref().or_else(|| {
            self.data
                .layout
                .grid_managers
                .iter()
                .find(|manager| manager.id == object.grid_manager)
                .and_then(|manager| manager.motion.as_deref())
        });
        let Some(motion_id) = motion else {
            return object.world;
        };
        let Some(transform_id) = object.motion_transform.as_deref() else {
            return self.transform_world(object.world, motion_id);
        };
        let motion = self.motion(motion_id).expect("validated gameplay animator");
        if transform_id == motion.transform {
            return self.transform_world(object.world, motion_id);
        }
        let runtime = self
            .motions
            .get(motion_id)
            .expect("initialized gameplay animator");
        let state = &motion.states[runtime.state];
        let Some(clip) = state
            .clip
            .as_ref()
            .and_then(|id| motion.clips.iter().find(|clip| clip.id == *id))
        else {
            return object.world;
        };
        let mut local = object.motion_local_position.unwrap_or_default();
        if let Some(tracks) = clip
            .transform_channels
            .get(transform_id)
            .and_then(|channels| channels.get("position"))
        {
            let time = self.motion_clip_time(runtime, state, clip);
            for (index, value) in [&mut local.x, &mut local.y, &mut local.z]
                .into_iter()
                .enumerate()
            {
                if let Some(track) = tracks.get(index).and_then(Option::as_ref) {
                    *value = motion_track_value(track, time);
                }
            }
        }
        let (root_position, root_rotation) = self.motion_root_world_pose(motion_id);
        vector_add(
            root_position,
            quaternion_rotate_vector(root_rotation, local),
        )
    }

    fn initialize_stations(&mut self) -> Result<(), String> {
        for object in &self.data.layout.objects {
            if let Some(feature) = object.feature("PlateReturnStation") {
                let clean = feature
                    .get("stackPrefab")
                    .and_then(Value::as_str)
                    .is_some_and(|name| name.contains("Clean"));
                let count = feature
                    .get("startingPlateNumber")
                    .and_then(Value::as_u64)
                    .unwrap_or(0) as usize;
                self.stacks
                    .insert(object.id.clone(), PlateStack { clean, count });
            }
            if let Some(feature) = object.feature("WashingStation") {
                let clean_ms =
                    self.scale_held_input(seconds_to_ms(number(feature, "cleanPlateTime")?));
                let drying_station = string(feature, "dryingStation")?;
                self.sinks.insert(
                    object.id.clone(),
                    Sink {
                        count: 0,
                        progress_ms: 0,
                        clean_ms,
                        drying_station,
                    },
                );
            }
        }
        for plate in &self.data.layout.initial_plates {
            let target = self.closest_slot(plate.world)?;
            if self.slots.contains_key(&target) {
                continue;
            }
            let item = self.item(
                plate.name.clone(),
                ItemBody::Plate {
                    contents: Vec::new(),
                },
            );
            self.slots.insert(target, item);
        }
        for utensil in &self.data.layout.cooking_utensils {
            let target = self.closest_slot(utensil.world)?;
            if self.slots.contains_key(&target) {
                continue;
            }
            let item = self.item(
                utensil.name.clone(),
                ItemBody::Container {
                    base_order: None,
                    capacity: utensil.container_capacity,
                    contents: Vec::new(),
                    cooking: Some(Cooking {
                        step: utensil.cooking_step.clone(),
                        station_type: utensil.station_type,
                        duration_ms: self.scale(seconds_to_ms(utensil.cooking_seconds)),
                        progress_ms: 0,
                    }),
                },
            );
            self.slots.insert(target, item);
        }
        for extinguisher in &self.data.layout.fire_extinguishers {
            let item = self.item(
                extinguisher.name.clone(),
                ItemBody::Extinguisher {
                    extinguish_ms: self
                        .scale_held_input(seconds_to_ms(extinguisher.extinguish_seconds)),
                    spray_distance: extinguisher.spray_distance,
                },
            );
            self.loose_items.insert(
                extinguisher.id.clone(),
                LooseItem {
                    grid_manager: extinguisher.grid_manager.clone(),
                    grid: extinguisher.grid,
                    world: extinguisher.world,
                    item,
                },
            );
        }
        Ok(())
    }

    fn initialize_hazards(&mut self) -> Result<(), String> {
        let managers = self
            .data
            .layout
            .systems
            .iter()
            .filter(|system| system.enabled && system.kind == "MeteorManager")
            .map(|system| system.id.clone())
            .collect::<Vec<_>>();
        for system in managers {
            let delay = self.next_meteor_delay(&system)?;
            self.meteor_managers.push(MeteorManagerRuntime {
                system,
                next_spawn_ms: delay,
            });
        }
        let spawners = self
            .data
            .layout
            .systems
            .iter()
            .filter(|system| system.enabled && system.kind == "FireballSpawner")
            .map(|system| {
                let motion_id = system
                    .motion
                    .as_deref()
                    .ok_or_else(|| format!("{} has no firing animation", system.id))?;
                let motion = self.motion(motion_id)?;
                let state = motion
                    .states
                    .get(motion.default_state)
                    .ok_or_else(|| format!("{motion_id} has no default state"))?;
                let clip = state
                    .clip
                    .as_ref()
                    .and_then(|id| motion.clips.iter().find(|clip| clip.id == *id))
                    .ok_or_else(|| format!("{motion_id} has no default firing clip"))?;
                let speed = state.speed.abs().max(0.000_001);
                let period_ms = self
                    .scale(seconds_to_ms(clip.duration_seconds / speed))
                    .max(1);
                let mut offsets_ms = clip
                    .properties
                    .values()
                    .filter_map(Option::as_ref)
                    .flat_map(firing_track_times)
                    .map(|seconds| self.scale(seconds_to_ms(seconds / speed)))
                    .filter(|offset| *offset < period_ms)
                    .collect::<Vec<_>>();
                offsets_ms.sort_unstable();
                offsets_ms.dedup();
                if offsets_ms.is_empty() {
                    return Err(format!("{} has no authored firing commands", system.id));
                }
                Ok(FireballSpawnerRuntime {
                    system: system.id.clone(),
                    period_ms,
                    next_spawn_ms: offsets_ms[0],
                    offsets_ms,
                    offset_index: 0,
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        self.fireball_spawners = spawners;
        Ok(())
    }

    fn next_meteor_delay(&mut self, id: &str) -> Result<u64, String> {
        let system = self
            .data
            .layout
            .systems
            .iter()
            .find(|system| system.id == id)
            .ok_or_else(|| format!("unknown meteor manager {id}"))?;
        let minimum = system
            .fields
            .get("m_minTimePeriod")
            .and_then(Value::as_f64)
            .ok_or_else(|| format!("{id} has no minimum period"))?;
        let maximum = system
            .fields
            .get("m_maxTimePeriod")
            .and_then(Value::as_f64)
            .ok_or_else(|| format!("{id} has no maximum period"))?;
        let delay = minimum + (maximum - minimum) * self.random_hazard_fraction();
        Ok(self.scale(seconds_to_ms(delay)))
    }

    fn process_hazard_due(&mut self) -> Result<(), String> {
        let due_managers = self
            .meteor_managers
            .iter()
            .filter(|manager| manager.next_spawn_ms <= self.elapsed_ms)
            .map(|manager| manager.system.clone())
            .collect::<Vec<_>>();
        for id in due_managers {
            let targets = self
                .data
                .layout
                .systems
                .iter()
                .find(|system| system.id == id)
                .map(|system| system.targets.clone())
                .ok_or_else(|| format!("unknown meteor manager {id}"))?;
            if !targets.is_empty() {
                let index = (self.random_hazard_fraction() * targets.len() as f64) as usize;
                let target = &targets[index.min(targets.len() - 1)];
                let meteor_id = format!("meteor-{}", self.next_hazard);
                self.next_hazard += 1;
                self.meteors.push(MeteorWarning {
                    id: meteor_id.clone(),
                    world: target.world,
                    impact_ms: self.elapsed_ms + self.scale(6_000),
                });
                self.event(
                    "meteor_warning",
                    format!(
                        "{meteor_id} targets grid {},{}.",
                        target.grid.x, target.grid.z
                    ),
                );
            }
            let next = self.elapsed_ms + self.next_meteor_delay(&id)?;
            if let Some(manager) = self
                .meteor_managers
                .iter_mut()
                .find(|manager| manager.system == id)
            {
                manager.next_spawn_ms = next;
            }
        }
        let impacts = self
            .meteors
            .iter()
            .filter(|meteor| meteor.impact_ms <= self.elapsed_ms)
            .map(|meteor| (meteor.id.clone(), meteor.world))
            .collect::<Vec<_>>();
        self.meteors
            .retain(|meteor| meteor.impact_ms > self.elapsed_ms);
        for (id, world) in impacts {
            let targets = self
                .data
                .layout
                .objects
                .iter()
                .filter(|object| {
                    object.has("Flammable") && distance(self.object_world(object), world) < 2.0
                })
                .map(|object| object.id.clone())
                .collect::<Vec<_>>();
            for target in targets {
                self.ignite(&target);
            }
            self.event("meteor_impact", format!("{id} impacted the kitchen."));
        }
        let due_spawners = self
            .fireball_spawners
            .iter()
            .filter(|spawner| spawner.next_spawn_ms <= self.elapsed_ms)
            .map(|spawner| spawner.system.clone())
            .collect::<Vec<_>>();
        for id in due_spawners {
            let system = self
                .data
                .layout
                .systems
                .iter()
                .find(|system| system.id == id)
                .ok_or_else(|| format!("unknown fireball spawner {id}"))?;
            let target = system
                .target_world
                .ok_or_else(|| format!("{id} has no fireball target"))?;
            let speed = system
                .fields
                .get("m_fireballSpeed")
                .and_then(Value::as_f64)
                .ok_or_else(|| format!("{id} has no fireball speed"))?;
            let duration_ms = self
                .scale(seconds_to_ms(distance(system.world, target) / speed))
                .max(1);
            let fireball_id = format!("fireball-{}", self.next_hazard);
            self.next_hazard += 1;
            self.fireballs.push(Fireball {
                id: fireball_id.clone(),
                from: system.world,
                to: target,
                spawned_ms: self.elapsed_ms,
                due_ms: self.elapsed_ms + duration_ms,
            });
            self.event(
                "fireball_fired",
                format!("{fireball_id} crossed the kitchen."),
            );

            let spawner = self
                .fireball_spawners
                .iter_mut()
                .find(|spawner| spawner.system == id)
                .expect("due fireball spawner exists");
            let current = spawner.offsets_ms[spawner.offset_index];
            spawner.offset_index = (spawner.offset_index + 1) % spawner.offsets_ms.len();
            let next = spawner.offsets_ms[spawner.offset_index];
            let gap = if next > current {
                next - current
            } else {
                spawner.period_ms - current + next
            };
            spawner.next_spawn_ms = spawner.next_spawn_ms.saturating_add(gap.max(1));
        }

        let collisions = self
            .fireballs
            .iter()
            .filter_map(|fireball| {
                self.fireball_collision(fireball)
                    .filter(|(due, _)| *due <= self.elapsed_ms)
                    .map(|(_, chef)| (fireball.id.clone(), chef))
            })
            .collect::<Vec<_>>();
        for (id, chef) in collisions {
            if self.fireballs.iter().any(|fireball| fireball.id == id) {
                self.fireballs.retain(|fireball| fireball.id != id);
                self.kill_chef(chef, false)?;
                self.event("fireball_hit", format!("{id} hit a chef."));
            }
        }
        let expired_fireballs = self
            .fireballs
            .iter()
            .filter(|fireball| fireball.due_ms <= self.elapsed_ms)
            .map(|fireball| fireball.id.clone())
            .collect::<HashSet<_>>();
        self.fireballs
            .retain(|fireball| !expired_fireballs.contains(&fireball.id));
        let sunken = self
            .chefs
            .iter()
            .enumerate()
            .filter(|(_, chef)| chef.respawn_due_ms.is_none())
            .filter_map(|(index, chef)| {
                let cell = &self.data.layout.walkable[chef.cell];
                if cell.motion.is_some() {
                    self.cell_world(chef.cell).y < cell.world.y - 0.65
                } else {
                    false
                }
                .then_some(index)
            })
            .collect::<Vec<_>>();
        for chef in sunken {
            self.kill_chef(chef, true)?;
        }
        let respawned = self
            .chefs
            .iter_mut()
            .filter(|chef| {
                chef.respawn_due_ms
                    .is_some_and(|due| due <= self.elapsed_ms)
            })
            .map(|chef| {
                chef.respawn_due_ms = None;
                chef.id
            })
            .collect::<Vec<_>>();
        for chef in respawned {
            self.event("chef_respawned", format!("Chef {chef} returned."));
        }
        Ok(())
    }

    fn fireball_collision(&self, fireball: &Fireball) -> Option<(u64, usize)> {
        self.chefs
            .iter()
            .enumerate()
            .filter(|(_, chef)| chef.respawn_due_ms.is_none())
            .filter_map(|(index, chef)| {
                fireball_collision_ms(fireball, self.cell_world(chef.cell), self.elapsed_ms)
                    .map(|due| (due, index))
            })
            .min_by_key(|(due, _)| *due)
    }

    fn kill_chef(&mut self, index: usize, fell: bool) -> Result<(), String> {
        if self.chefs[index].respawn_due_ms.is_some() {
            return Ok(());
        }
        self.works[index] = None;
        let chef_world = self.cell_world(self.chefs[index].cell);
        let held = self.chefs[index].held.take();
        let mut dropped_at = None;
        if !fell && let Some(item) = held.as_ref() {
            let mut candidates = self
                .data
                .layout
                .objects
                .iter()
                .filter(|object| object.has("AttachStation"))
                .filter(|object| !self.slots.contains_key(&object.id))
                .filter(|object| !self.fires.contains_key(&object.id))
                .filter(|object| self.check_placement(&object.id, item).is_ok())
                .map(|object| {
                    (
                        object.id.clone(),
                        distance(chef_world, self.object_world(object)),
                    )
                })
                .collect::<Vec<_>>();
            candidates.sort_by(|left, right| left.1.total_cmp(&right.1));
            dropped_at = candidates
                .first()
                .filter(|(_, distance)| *distance <= INTERACTION_DISTANCE)
                .map(|(id, _)| id.clone());
        }
        if let Some(item) = held {
            if let Some(target) = dropped_at.as_ref() {
                self.slots.insert(target.clone(), item);
                self.event(
                    "item_dropped",
                    format!("The struck chef dropped the held item on {target}."),
                );
            } else if !fell {
                let cell = &self.data.layout.walkable[self.chefs[index].cell];
                let id = item.id.clone();
                self.loose_items.insert(
                    id.clone(),
                    LooseItem {
                        grid_manager: cell.grid_manager.clone(),
                        grid: cell_coordinate(cell),
                        world: chef_world,
                        item,
                    },
                );
                self.event(
                    "item_dropped",
                    format!("The struck chef dropped the held item as {id}."),
                );
            }
        }
        let chef_id = self.chefs[index].id;
        self.chefs[index].travel = None;
        let spawn = self
            .data
            .layout
            .players
            .iter()
            .find(|spawn| spawn.id == chef_id)
            .ok_or_else(|| format!("chef {chef_id} has no authored spawn"))?;
        self.chefs[index].cell = self.chefs[index].spawn_cell;
        self.chefs[index].respawn_due_ms = Some(
            self.elapsed_ms
                + self.scale(seconds_to_ms(
                    spawn.respawn_seconds + spawn.spawn_effect_seconds,
                )),
        );
        self.event(
            "chef_down",
            format!(
                "Chef {chef_id} {} and is respawning.",
                if fell { "fell" } else { "was hit" }
            ),
        );
        Ok(())
    }

    fn ignite(&mut self, target: &str) {
        if self.data.variant.config.fire.is_none()
            || self.fires.contains_key(target)
            || !self
                .data
                .layout
                .objects
                .iter()
                .any(|object| object.id == target && object.has("Flammable"))
        {
            return;
        }
        self.fires.insert(
            target.to_owned(),
            FireState {
                strength: 1.0,
                recovery_suppressed_ms: 0,
            },
        );
        self.fire_exposure_ms.remove(target);
        self.event("fire_ignited", format!("{target} caught fire."));
    }

    fn advance_fire(&mut self, delta: u64) {
        let Some(config) = self.data.variant.config.fire.as_ref() else {
            return;
        };
        let sprayed = self
            .works
            .iter()
            .flatten()
            .filter_map(|work| match work {
                Work::Extinguish { target, .. } => Some(target.as_str()),
                _ => None,
            })
            .collect::<HashSet<_>>();
        let recovery = self.scale(seconds_to_ms(config.recovery_seconds)).max(1);
        let suppression = self.scale(seconds_to_ms(config.encouragement_suppressed_seconds));
        for (id, fire) in &mut self.fires {
            if fire.strength >= 1.0 {
                continue;
            }
            if sprayed.contains(id.as_str()) {
                fire.recovery_suppressed_ms = suppression;
                continue;
            }
            let recovering_ms = delta.saturating_sub(fire.recovery_suppressed_ms);
            fire.recovery_suppressed_ms = fire.recovery_suppressed_ms.saturating_sub(delta);
            fire.strength = (fire.strength + recovering_ms as f64 / recovery as f64).min(1.0);
        }
        let active = self
            .data
            .layout
            .objects
            .iter()
            .filter(|object| self.fires.contains_key(&object.id))
            .map(|object| {
                (
                    object.grid_manager.clone(),
                    object.grid.x,
                    object.grid.y,
                    object.grid.z,
                )
            })
            .collect::<Vec<_>>();
        let threshold = self.scale(seconds_to_ms(config.flammability_seconds));
        let cooldown = self.scale(seconds_to_ms(config.cooldown_seconds)).max(1);
        let mut ignite = Vec::new();
        for object in self
            .data
            .layout
            .objects
            .iter()
            .filter(|object| object.has("Flammable") && !self.fires.contains_key(&object.id))
        {
            let encouraged = active.iter().any(|(manager, x, y, z)| {
                *manager == object.grid_manager
                    && (*x - object.grid.x).abs() <= 1
                    && *y == object.grid.y
                    && (*z - object.grid.z).abs() <= 1
            });
            let progress = self.fire_exposure_ms.entry(object.id.clone()).or_default();
            if encouraged {
                *progress = progress.saturating_add(delta);
            } else {
                *progress = progress.saturating_sub(
                    ((delta as u128 * threshold as u128) / cooldown as u128) as u64,
                );
            }
            if *progress >= threshold {
                ignite.push(object.id.clone());
            }
        }
        for target in ignite {
            self.ignite(&target);
        }
    }

    fn refresh_conveyors(&mut self) -> Result<(), String> {
        if !self.started {
            return Ok(());
        }
        let conveyors = self
            .data
            .layout
            .systems
            .iter()
            .filter(|system| system.kind == "ConveyorStation")
            .filter_map(|system| {
                let source = system.object.clone()?;
                let target = system.target_object.clone()?;
                let speed = system.fields.get("m_conveySpeed")?.as_f64()?;
                (speed > 0.0).then_some((source, target, speed))
            })
            .collect::<Vec<_>>();
        for (source, target, speed) in conveyors {
            if self.conveyor_transfers.iter().any(|transfer| {
                transfer.source == source || transfer.target == source || transfer.target == target
            }) || !self.slots.contains_key(&source)
                || self.slots.contains_key(&target)
            {
                continue;
            }
            let item = self.slots.get(&source).expect("checked conveyor item");
            if self.check_placement(&target, item).is_err() {
                continue;
            }
            let duration_ms = self.scale(seconds_to_ms(1.0 / speed)).max(2);
            self.conveyor_transfers.push(ConveyorTransfer {
                source: source.clone(),
                target: target.clone(),
                started_ms: self.elapsed_ms,
                midpoint_ms: self.elapsed_ms + duration_ms / 2,
                due_ms: self.elapsed_ms + duration_ms,
                crossed_midpoint: false,
            });
            self.event(
                "conveyor_started",
                format!("{source} started conveying an item to {target}."),
            );
        }
        Ok(())
    }

    fn process_conveyor_due(&mut self) -> Result<(), String> {
        let mut cancelled = HashSet::new();
        let mut crossed = Vec::new();
        for index in 0..self.conveyor_transfers.len() {
            if self.conveyor_transfers[index].crossed_midpoint
                || self.conveyor_transfers[index].midpoint_ms > self.elapsed_ms
            {
                continue;
            }
            let source = self.conveyor_transfers[index].source.clone();
            let target = self.conveyor_transfers[index].target.clone();
            let Some(item) = self.slots.remove(&source) else {
                cancelled.insert(index);
                continue;
            };
            if self.slots.contains_key(&target) {
                self.slots.insert(source, item);
                cancelled.insert(index);
                continue;
            }
            self.slots.insert(target.clone(), item);
            self.conveyor_transfers[index].crossed_midpoint = true;
            crossed.push(target);
        }
        if !cancelled.is_empty() {
            self.conveyor_transfers = self
                .conveyor_transfers
                .drain(..)
                .enumerate()
                .filter_map(|(index, transfer)| (!cancelled.contains(&index)).then_some(transfer))
                .collect();
        }
        for target in crossed {
            self.event(
                "conveyor_midpoint",
                format!("The conveyor transferred item ownership to {target}."),
            );
        }
        let completed = self
            .conveyor_transfers
            .iter()
            .filter(|transfer| transfer.due_ms <= self.elapsed_ms)
            .map(|transfer| transfer.target.clone())
            .collect::<Vec<_>>();
        self.conveyor_transfers
            .retain(|transfer| transfer.due_ms > self.elapsed_ms);
        for target in completed {
            self.event("conveyor_completed", format!("The item reached {target}."));
        }
        Ok(())
    }

    fn cancel_conveyor_for_item_at(&mut self, location: &str) {
        self.conveyor_transfers.retain(|transfer| {
            !((!transfer.crossed_midpoint && transfer.source == location)
                || (transfer.crossed_midpoint && transfer.target == location))
        });
    }

    fn refresh_trigger_zones(&mut self) -> Result<(), String> {
        if !self.started {
            return Ok(());
        }
        let changes = self
            .data
            .layout
            .systems
            .iter()
            .filter(|system| system.kind == "TriggerZone")
            .filter_map(|system| {
                let occupied = self.chefs.iter().any(|chef| {
                    let world = self.cell_world(chef.cell);
                    (world.x - system.world.x).hypot(world.z - system.world.z) < 0.7
                        && (world.y - system.world.y).abs() < 0.7
                });
                if occupied == self.occupied_zones.contains(&system.id) {
                    return None;
                }
                let field = if occupied {
                    "m_onOccupationTrigger"
                } else {
                    "m_onDeoccupationTrigger"
                };
                Some((
                    system.id.clone(),
                    system.name.clone(),
                    occupied,
                    system.fields.get(field)?.as_str()?.to_owned(),
                ))
            })
            .collect::<Vec<_>>();
        for (id, name, occupied, trigger) in changes {
            if occupied {
                self.occupied_zones.insert(id);
            } else {
                self.occupied_zones.remove(&id);
            }
            self.dispatch_object_trigger(&name, &trigger)?;
            self.event(
                if occupied {
                    "pressure_switch_entered"
                } else {
                    "pressure_switch_exited"
                },
                format!("{name} fired {trigger}."),
            );
        }
        Ok(())
    }

    fn closest_slot(&self, world: Vector3) -> Result<String, String> {
        self.data
            .layout
            .objects
            .iter()
            .filter(|object| object.has("AttachStation"))
            .min_by(|left, right| {
                distance(left.world, world).total_cmp(&distance(right.world, world))
            })
            .filter(|object| distance(object.world, world) <= INTERACTION_DISTANCE)
            .map(|object| object.id.clone())
            .ok_or_else(|| "initial item has no authored surface".to_owned())
    }

    fn take_from_spawner(&mut self, target: &str) -> Result<(), String> {
        if self.active_chef().held.is_some() {
            return Err("the active chef's hands are full".to_owned());
        }
        let feature = self
            .object(target)?
            .feature("PickupItemSpawner")
            .ok_or_else(|| "spawner data is missing".to_owned())?
            .clone();
        let name = string(&feature, "ingredient")?;
        let properties = food_properties(
            feature
                .get("ingredient_properties")
                .ok_or_else(|| "spawner ingredient properties are missing".to_owned())?,
            self.config.time_scale,
        )?;
        let process = match feature.get("processed_item").and_then(Value::as_str) {
            Some(result_name) => {
                let stages = feature
                    .get("work_stages")
                    .and_then(Value::as_u64)
                    .ok_or_else(|| "work stages are missing".to_owned())?;
                let impacts = stages.saturating_sub(1).saturating_mul(u64::from(
                    self.data.campaign.scoring.single_player_chops_per_stage,
                ));
                let source_ms = (self.data.campaign.scoring.chop_impact_seconds * 1_000.0).round()
                    as u64
                    * impacts;
                Some(ProcessSpec {
                    result_name: result_name.to_owned(),
                    result: food_properties(
                        feature.get("processed_properties").ok_or_else(|| {
                            "processed ingredient properties are missing".to_owned()
                        })?,
                        self.config.time_scale,
                    )?,
                    required_ms: self.scale_held_input(source_ms),
                })
            }
            None => None,
        };
        let body = item_from_properties(properties, process);
        let item = self.item(name, body);
        self.active_chef_mut().held = Some(item);
        Ok(())
    }

    fn place_dirty_stack(&mut self, target: &str) -> Result<(), String> {
        let held = self
            .active_chef_mut()
            .held
            .take()
            .ok_or_else(|| "the active chef is not holding dirty plates".to_owned())?;
        let ItemBody::DirtyPlateStack { count } = held.body else {
            self.active_chef_mut().held = Some(held);
            return Err("only a dirty plate stack can be put in the sink".to_owned());
        };
        let sink = self
            .sinks
            .get_mut(target)
            .ok_or_else(|| "unknown washing station".to_owned())?;
        sink.count += count;
        Ok(())
    }

    fn take_from_plate_stack(&mut self, target: &str) -> Result<(), String> {
        if self.active_chef().held.is_some() {
            return Err("the active chef's hands are full".to_owned());
        }
        let (clean, count) = {
            let stack = self
                .stacks
                .get_mut(target)
                .ok_or_else(|| "unknown plate return".to_owned())?;
            if stack.count == 0 {
                return Err("the plate return is empty".to_owned());
            }
            if stack.clean {
                stack.count -= 1;
                (true, 1)
            } else {
                let count = stack.count;
                stack.count = 0;
                (false, count)
            }
        };
        let item = if clean {
            self.item(
                "Plate".to_owned(),
                ItemBody::Plate {
                    contents: Vec::new(),
                },
            )
        } else {
            self.item(
                "Dirty plate stack".to_owned(),
                ItemBody::DirtyPlateStack { count },
            )
        };
        self.active_chef_mut().held = Some(item);
        Ok(())
    }

    fn interact_with_slot(&mut self, target: &str) -> Result<(), String> {
        if !self.object(target)?.has("AttachStation") {
            return Err("target is not an interactive surface".to_owned());
        }
        if self.active_chef().held.is_some()
            && self
                .conveyor_transfers
                .iter()
                .any(|transfer| transfer.target == target)
        {
            return Err("the surface is receiving an item from a conveyor".to_owned());
        }
        if self.active_chef().held.is_none() && self.slots.contains_key(target) {
            self.cancel_conveyor_for_item_at(target);
        }
        let held = self.active_chef_mut().held.take();
        let slot = self.slots.remove(target);
        let result = match (held, slot) {
            (None, None) => Err("the surface is empty".to_owned()),
            (None, Some(item)) => {
                if self.object(target)?.has("Workstation")
                    && matches!(
                        item.body,
                        ItemBody::Food {
                            process: Some(_),
                            work_progress_ms,
                            ..
                        } if work_progress_ms > 0
                    )
                {
                    self.slots.insert(target.to_owned(), item);
                    return Err("partly processed food cannot leave the workstation".to_owned());
                }
                self.active_chef_mut().held = Some(item);
                Ok(())
            }
            (Some(item), None) => {
                if let Err(error) = self.check_placement(target, &item) {
                    self.active_chef_mut().held = Some(item);
                    return Err(error);
                }
                self.slots.insert(target.to_owned(), item);
                Ok(())
            }
            (Some(mut held), Some(mut placed)) => {
                let result = self.combine(&mut held, &mut placed);
                match result {
                    Ok(CombineResult::Held) => {
                        self.slots.insert(target.to_owned(), placed);
                    }
                    Ok(CombineResult::Placed) => {
                        self.active_chef_mut().held = Some(held);
                    }
                    Ok(CombineResult::Neither) => {
                        self.active_chef_mut().held = Some(held);
                        self.slots.insert(target.to_owned(), placed);
                    }
                    Err(error) => {
                        self.active_chef_mut().held = Some(held);
                        self.slots.insert(target.to_owned(), placed);
                        return Err(error);
                    }
                }
                Ok(())
            }
        };
        if result.is_ok() {
            self.refresh_conveyors()?;
        }
        result
    }

    fn check_placement(&self, target: &str, item: &Item) -> Result<(), String> {
        let object = self.object(target)?;
        if object.has("Workstation")
            && !matches!(
                item.body,
                ItemBody::Food {
                    process: Some(_),
                    ..
                }
            )
        {
            return Err("only processable raw food can go on a workstation".to_owned());
        }
        if let Some(feature) = object.feature("CookingStation") {
            let station_type = feature
                .get("stationType")
                .and_then(Value::as_i64)
                .ok_or_else(|| "cooking station type is missing".to_owned())?;
            let ItemBody::Container {
                cooking: Some(cooking),
                ..
            } = &item.body
            else {
                return Err("that item cannot be placed on this cooker".to_owned());
            };
            if cooking.station_type != station_type {
                return Err("the cooking vessel requires a different cooker".to_owned());
            }
        }
        Ok(())
    }

    fn combine(&self, held: &mut Item, placed: &mut Item) -> Result<CombineResult, String> {
        match (&mut held.body, &mut placed.body) {
            (ItemBody::Food { order, .. }, ItemBody::Plate { contents }) => {
                contents.push(
                    order
                        .clone()
                        .ok_or_else(|| "raw food must be processed first".to_owned())?,
                );
                Ok(CombineResult::Held)
            }
            (ItemBody::Plate { contents }, ItemBody::Food { order, .. }) => {
                contents.push(
                    order
                        .clone()
                        .ok_or_else(|| "raw food must be processed first".to_owned())?,
                );
                Ok(CombineResult::Placed)
            }
            (
                ItemBody::Food { order, .. },
                ItemBody::Container {
                    capacity,
                    contents,
                    cooking,
                    ..
                },
            ) => {
                add_to_container(
                    contents,
                    *capacity,
                    order
                        .clone()
                        .ok_or_else(|| "raw food must be processed first".to_owned())?,
                    cooking,
                )?;
                Ok(CombineResult::Held)
            }
            (ItemBody::Container { .. }, ItemBody::Plate { contents }) => {
                let tokens = self.container_tokens(held)?;
                contents.extend(tokens);
                Ok(if clear_if_utensil(held) {
                    CombineResult::Neither
                } else {
                    CombineResult::Held
                })
            }
            (ItemBody::Plate { contents }, ItemBody::Container { .. }) => {
                let tokens = self.container_tokens(placed)?;
                contents.extend(tokens);
                Ok(if clear_if_utensil(placed) {
                    CombineResult::Neither
                } else {
                    CombineResult::Placed
                })
            }
            (
                ItemBody::Container { .. },
                ItemBody::Container {
                    capacity,
                    contents,
                    cooking,
                    ..
                },
            ) => {
                let tokens = self.container_tokens(held)?;
                if tokens.len() != 1 {
                    return Err("that prepared item cannot be combined here".to_owned());
                }
                add_to_container(contents, *capacity, tokens[0].clone(), cooking)?;
                Ok(if clear_if_utensil(held) {
                    CombineResult::Neither
                } else {
                    CombineResult::Held
                })
            }
            _ => Err("those items cannot be combined".to_owned()),
        }
    }

    fn container_tokens(&self, item: &Item) -> Result<Vec<String>, String> {
        let ItemBody::Container {
            base_order,
            contents,
            cooking,
            ..
        } = &item.body
        else {
            return Err("item is not a container".to_owned());
        };
        let mut raw = contents.clone();
        if let Some(base) = base_order {
            raw.push(base.clone());
        }
        if let Some(cooking) = cooking {
            if cooking.progress_ms > cooking.duration_ms.saturating_mul(2) {
                return Err("the food is burnt".to_owned());
            }
            if cooking.progress_ms < cooking.duration_ms {
                return Err("the food is not cooked".to_owned());
            }
            let cooked = self
                .resolve_cooked(&raw, &cooking.step)
                .ok_or_else(|| "the cooked contents do not form a recipe".to_owned())?;
            return Ok(vec![cooked]);
        }
        if raw.is_empty() {
            return Err("the container is empty".to_owned());
        }
        Ok(raw)
    }

    fn deliver(&mut self, target: &str) -> Result<(), String> {
        let held = self
            .active_chef_mut()
            .held
            .take()
            .ok_or_else(|| "delivery requires a plated meal".to_owned())?;
        let ItemBody::Plate { contents } = held.body else {
            self.active_chef_mut().held = Some(held);
            return Err("delivery requires a plate".to_owned());
        };
        let matching = self
            .orders
            .iter()
            .enumerate()
            .filter(|(_, order)| {
                order
                    .entry
                    .order
                    .as_deref()
                    .is_some_and(|recipe| self.matches_recipe(recipe, &contents))
            })
            .min_by_key(|(_, order)| order.deadline_ms)
            .map(|(index, _)| index);
        if let Some(index) = matching {
            let order = self.orders.remove(index);
            let lifetime = self.order_lifetime_ms();
            let remaining_fraction =
                order.deadline_ms.saturating_sub(self.elapsed_ms) as f64 / lifetime as f64;
            let tip = self
                .data
                .campaign
                .scoring
                .tip_boundaries
                .iter()
                .filter(|boundary| boundary.remaining_fraction_exclusive_min < remaining_fraction)
                .max_by(|left, right| {
                    left.remaining_fraction_exclusive_min
                        .total_cmp(&right.remaining_fraction_exclusive_min)
                })
                .map_or(0, |boundary| boundary.points);
            let earned = self.data.campaign.scoring.default_delivery_points + tip;
            self.score += earned;
            self.event(
                "order_delivered",
                format!(
                    "{} completed for {earned} points.",
                    order.entry.order.unwrap_or_default()
                ),
            );
        } else {
            self.event(
                "wrong_delivery",
                "The plated meal did not match an active order.",
            );
        }
        let return_station = self.delivery_return_station(target)?;
        self.pending_plates.push(PendingPlate {
            due_ms: self.elapsed_ms
                + self.scale(seconds_to_ms(self.data.variant.config.plate_return_seconds)),
            station: return_station,
        });
        self.process_due()?;
        Ok(())
    }

    fn delivery_return_station(&self, delivery: &str) -> Result<String, String> {
        if let Some(id) = self
            .object(delivery)?
            .feature("PlateStation")
            .and_then(|feature| feature.get("returnStation"))
            .and_then(Value::as_str)
        {
            return Ok(id.to_owned());
        }
        self.stacks
            .iter()
            .find(|(_, stack)| !stack.clean)
            .or_else(|| self.stacks.iter().next())
            .map(|(id, _)| id.clone())
            .ok_or_else(|| "level has no plate return".to_owned())
    }

    fn resolve_cooked(&self, contents: &[String], step: &str) -> Option<String> {
        self.data
            .campaign
            .orders
            .iter()
            .find(|node| {
                node.kind == "cooked_composite"
                    && node.cooking_step.as_deref() == Some(step)
                    && same_multiset(&node.required, contents)
            })
            .map(|node| node.id.clone())
    }

    fn cooking_ready(&self, id: &str, station_type: i64) -> bool {
        let Some(Item {
            body:
                ItemBody::Container {
                    base_order,
                    contents,
                    cooking: Some(cooking),
                    ..
                },
            ..
        }) = self.slots.get(id)
        else {
            return false;
        };
        if cooking.station_type != station_type {
            return false;
        }
        let mut ingredients = contents.clone();
        ingredients.extend(base_order.iter().cloned());
        self.resolve_cooked(&ingredients, &cooking.step).is_some()
    }

    fn matches_recipe(&self, recipe: &str, contents: &[String]) -> bool {
        let Some(node) = self
            .data
            .campaign
            .orders
            .iter()
            .find(|node| node.id == recipe)
        else {
            return false;
        };
        match node.kind.as_str() {
            "ingredient" | "cooked_composite" => contents.len() == 1 && contents[0] == node.id,
            "composite" => same_multiset(&node.required, contents),
            _ => false,
        }
    }

    fn next_boundary(&self, target: u64) -> u64 {
        let mut next = target.min(self.duration_ms());
        if self.next_order_ms > self.elapsed_ms {
            next = next.min(self.next_order_ms);
        }
        for order in &self.orders {
            if order.deadline_ms > self.elapsed_ms {
                next = next.min(order.deadline_ms);
            }
        }
        for pending in &self.pending_plates {
            if pending.due_ms > self.elapsed_ms {
                next = next.min(pending.due_ms);
            }
        }
        for manager in &self.meteor_managers {
            if manager.next_spawn_ms > self.elapsed_ms {
                next = next.min(manager.next_spawn_ms);
            }
        }
        for meteor in &self.meteors {
            if meteor.impact_ms > self.elapsed_ms {
                next = next.min(meteor.impact_ms);
            }
        }
        for spawner in &self.fireball_spawners {
            if spawner.next_spawn_ms > self.elapsed_ms {
                next = next.min(spawner.next_spawn_ms);
            }
        }
        for fireball in &self.fireballs {
            if fireball.due_ms > self.elapsed_ms {
                next = next.min(fireball.due_ms);
            }
            if let Some((due, _)) = self.fireball_collision(fireball)
                && due > self.elapsed_ms
            {
                next = next.min(due);
            }
        }
        for chef in &self.chefs {
            if let Some(due_ms) = chef.respawn_due_ms
                && due_ms > self.elapsed_ms
            {
                next = next.min(due_ms);
            }
            if let Some(travel) = &chef.travel
                && travel.due_ms > self.elapsed_ms
            {
                next = next.min(travel.due_ms);
            }
        }
        if let Some(BossTransition::Intermission { due_ms }) = &self.boss_transition
            && *due_ms > self.elapsed_ms
        {
            next = next.min(*due_ms);
        }
        for transfer in &self.conveyor_transfers {
            if !transfer.crossed_midpoint && transfer.midpoint_ms > self.elapsed_ms {
                next = next.min(transfer.midpoint_ms);
            }
            if transfer.due_ms > self.elapsed_ms {
                next = next.min(transfer.due_ms);
            }
        }
        for work in self.works.iter().flatten() {
            if let Some(remaining) = self
                .work_remaining_ms(work)
                .filter(|remaining| *remaining > 0)
            {
                next = next.min(self.elapsed_ms.saturating_add(remaining));
            }
        }
        for (id, station_type) in &self.cooking_stations {
            if !self.cooking_ready(id, *station_type) {
                continue;
            }
            let Some(Item {
                body:
                    ItemBody::Container {
                        cooking: Some(cooking),
                        ..
                    },
                ..
            }) = self.slots.get(id)
            else {
                continue;
            };
            let ignition_progress = cooking.duration_ms.saturating_mul(2).saturating_add(1);
            if cooking.progress_ms < ignition_progress {
                next = next.min(
                    self.elapsed_ms
                        .saturating_add(ignition_progress - cooking.progress_ms),
                );
            }
        }
        if let Some(config) = self.data.variant.config.fire.as_ref()
            && !self.fires.is_empty()
        {
            let active = self
                .data
                .layout
                .objects
                .iter()
                .filter(|object| self.fires.contains_key(&object.id))
                .map(|object| {
                    (
                        object.grid_manager.as_str(),
                        object.grid.x,
                        object.grid.y,
                        object.grid.z,
                    )
                })
                .collect::<Vec<_>>();
            let threshold = self.scale(seconds_to_ms(config.flammability_seconds));
            for object in
                self.data.layout.objects.iter().filter(|object| {
                    object.has("Flammable") && !self.fires.contains_key(&object.id)
                })
            {
                let encouraged = active.iter().any(|(manager, x, y, z)| {
                    *manager == object.grid_manager
                        && (*x - object.grid.x).abs() <= 1
                        && *y == object.grid.y
                        && (*z - object.grid.z).abs() <= 1
                });
                let progress = self.fire_exposure_ms.get(&object.id).copied().unwrap_or(0);
                if encouraged && progress < threshold {
                    next = next.min(
                        self.elapsed_ms
                            .saturating_add(threshold.saturating_sub(progress).max(1)),
                    );
                }
            }
        }
        for (id, runtime) in &self.motions {
            for scheduled in &runtime.scheduled {
                if scheduled.due_ms > self.elapsed_ms {
                    next = next.min(scheduled.due_ms);
                }
            }
            if let Ok(motion) = self.motion(id)
                && let Some(state) = motion.states.get(runtime.state)
            {
                let duration = self.motion_state_duration_ms(motion, state);
                for transition in &state.transitions {
                    if transition.has_exit_time && transition.conditions.is_empty() {
                        let due = runtime.state_started_ms.saturating_add(
                            (duration as f64 * transition.exit_time.max(0.0)).round() as u64,
                        );
                        if due > self.elapsed_ms {
                            next = next.min(due);
                        }
                    }
                }
            }
        }
        next.max(self.elapsed_ms + 1).min(target)
    }

    fn advance_continuous(&mut self, delta: u64) {
        if delta == 0 {
            return;
        }
        self.advance_fire(delta);
        let mut burning = Vec::new();
        for (id, station_type) in &self.cooking_stations {
            if !self.cooking_ready(id, *station_type) {
                continue;
            }
            if let Some(Item {
                body:
                    ItemBody::Container {
                        cooking: Some(cooking),
                        ..
                    },
                ..
            }) = self.slots.get_mut(id)
            {
                let was_burnt = cooking.progress_ms > cooking.duration_ms.saturating_mul(2);
                cooking.progress_ms = cooking
                    .progress_ms
                    .saturating_add(delta)
                    .min(cooking.duration_ms.saturating_mul(2).saturating_add(1));
                if !was_burnt && cooking.progress_ms > cooking.duration_ms.saturating_mul(2) {
                    burning.push(id.clone());
                }
            }
        }
        for id in burning {
            self.ignite(&id);
        }
        for chef in 0..self.works.len() {
            self.advance_work(chef, delta);
        }
    }

    fn advance_work(&mut self, chef: usize, delta: u64) {
        let work = self.works[chef].clone();
        match work {
            Some(Work::Chop { target, .. }) => {
                let mut finished = None;
                if let Some(Item {
                    name,
                    body:
                        ItemBody::Food {
                            process: Some(process),
                            work_progress_ms,
                            ..
                        },
                    ..
                }) = self.slots.get_mut(&target)
                {
                    *work_progress_ms = work_progress_ms.saturating_add(delta);
                    if *work_progress_ms >= process.required_ms {
                        finished = Some((
                            name.clone(),
                            process.result_name.clone(),
                            process.result.clone(),
                        ));
                    }
                }
                if let Some((_old, result_name, properties)) = finished {
                    if let Some(item) = self.slots.get_mut(&target) {
                        item.name = result_name;
                        item.body = item_from_properties(properties, None);
                    }
                    self.works[chef] = None;
                    self.event(
                        "food_processed",
                        format!("Chef {} finished processing food.", self.chefs[chef].id),
                    );
                }
            }
            Some(Work::Wash { target, .. }) => {
                let mut cleaned = 0;
                let mut drying = None;
                if let Some(sink) = self.sinks.get_mut(&target) {
                    sink.progress_ms = sink.progress_ms.saturating_add(delta);
                    while sink.count > 0 && sink.progress_ms >= sink.clean_ms {
                        sink.progress_ms -= sink.clean_ms;
                        sink.count -= 1;
                        cleaned += 1;
                    }
                    drying = Some(sink.drying_station.clone());
                    if sink.count == 0 {
                        sink.progress_ms = 0;
                        self.works[chef] = None;
                    }
                }
                if cleaned > 0 {
                    if let Some(stack) = drying.and_then(|id| self.stacks.get_mut(&id)) {
                        stack.count += cleaned;
                    }
                    self.event(
                        "plates_washed",
                        format!("Chef {} washed {cleaned} plate(s).", self.chefs[chef].id),
                    );
                }
            }
            Some(Work::Extinguish { target, .. }) => {
                let required_ms = self.chefs[chef]
                    .held
                    .as_ref()
                    .and_then(|item| match item.body {
                        ItemBody::Extinguisher { extinguish_ms, .. } => Some(extinguish_ms),
                        _ => None,
                    })
                    .unwrap_or(1);
                let mut extinguished = false;
                if let Some(fire) = self.fires.get_mut(&target) {
                    fire.strength = (fire.strength - delta as f64 / required_ms as f64).max(0.0);
                    extinguished = fire.strength == 0.0;
                }
                if extinguished {
                    self.fires.remove(&target);
                    self.works[chef] = None;
                    self.event(
                        "fire_extinguished",
                        format!("Chef {} extinguished {target}.", self.chefs[chef].id),
                    );
                }
            }
            _ => {}
        }
    }

    fn process_boss_due(&mut self) -> Result<(), String> {
        let Some(flow) = self.data.layout.boss_flow.as_ref() else {
            return Ok(());
        };
        let transition = self.boss_transition.clone();
        match transition {
            Some(BossTransition::Intermission { due_ms }) if due_ms <= self.elapsed_ms => {
                let platform = flow
                    .platforms
                    .get(self.boss_phase)
                    .cloned()
                    .ok_or_else(|| "boss phase has no platform".to_owned())?;
                self.set_motion_bool(&platform, "Down", false)?;
                self.boss_transition = Some(BossTransition::Raising { platform });
            }
            Some(BossTransition::Raising { platform }) if self.motion_bool(&platform, "IsUp") => {
                self.boss_phase += 1;
                self.boss_phase_index = 0;
                let platform = flow
                    .platforms
                    .get(self.boss_phase)
                    .cloned()
                    .ok_or_else(|| "boss phase has no next platform".to_owned())?;
                self.set_motion_bool(&platform, "Down", true)?;
                self.boss_transition = Some(BossTransition::Lowering { platform });
            }
            Some(BossTransition::Lowering { platform })
                if self.motion_bool(&platform, "IsDown") =>
            {
                self.boss_transition = None;
                self.boss_ready = true;
                self.event(
                    "boss_phase_ready",
                    format!("Boss kitchen phase {} is ready.", self.boss_phase + 1),
                );
            }
            _ => {}
        }

        if self.boss_transition.is_none()
            && self.boss_ready
            && self.orders.is_empty()
            && self
                .data
                .variant
                .config
                .phases
                .get(self.boss_phase)
                .is_some_and(|phase| self.boss_phase_index >= phase.len())
        {
            self.boss_ready = false;
            if self.boss_phase + 1 >= self.data.variant.config.phases.len() {
                self.boss_complete = true;
                self.event("boss_completed", "Every boss kitchen phase was completed.");
            } else {
                let due_ms = self.elapsed_ms + self.scale(seconds_to_ms(flow.intermission_seconds));
                self.boss_transition = Some(BossTransition::Intermission { due_ms });
                self.event(
                    "boss_intermission",
                    format!("The next kitchen arrives after {due_ms} ms."),
                );
            }
        }
        Ok(())
    }

    fn motion_bool(&self, motion: &str, name: &str) -> bool {
        matches!(
            self.motions
                .get(motion)
                .and_then(|runtime| runtime.values.get(name)),
            Some(MotionValue::Bool(true))
        )
    }

    fn process_due(&mut self) -> Result<(), String> {
        self.process_motion_due()?;
        self.process_conveyor_due()?;
        self.process_hazard_due()?;
        self.process_travel_due()?;
        self.refresh_trigger_zones()?;
        self.process_boss_due()?;
        let lifetime = self.order_lifetime_ms();
        let expired = self
            .orders
            .iter_mut()
            .filter(|order| order.deadline_ms <= self.elapsed_ms)
            .map(|order| {
                order.deadline_ms = order.deadline_ms.saturating_add(lifetime);
                order.id.clone()
            })
            .collect::<Vec<_>>();
        for id in expired {
            self.score -= self.data.campaign.scoring.expired_order_penalty;
            self.event(
                "order_overdue",
                format!("{id} lost points and its timer restarted."),
            );
        }
        let mut returned = Vec::new();
        self.pending_plates.retain(|pending| {
            if pending.due_ms <= self.elapsed_ms {
                returned.push(pending.station.clone());
                false
            } else {
                true
            }
        });
        for station in returned {
            if let Some(stack) = self.stacks.get_mut(&station) {
                stack.count += 1;
            }
        }
        while self.orders.len() < self.data.layout.order_capacity
            && (self.data.layout.boss_flow.is_none() || self.boss_ready)
            && (self.elapsed_ms >= self.next_order_ms
                || (self.orders.len() < 2 && self.elapsed_ms > self.order_interval_ms()))
        {
            let Some(entry) = self.next_recipe() else {
                break;
            };
            let id = format!("order-{}", self.issued_orders + 1);
            let deadline_ms = self.elapsed_ms + self.order_lifetime_ms();
            self.orders.push(ActiveOrder {
                id: id.clone(),
                entry,
                opened_ms: self.elapsed_ms,
                deadline_ms,
            });
            self.issued_orders += 1;
            self.next_order_ms = self.elapsed_ms + self.order_interval_ms();
            self.event("order_opened", format!("{id} entered the queue."));
            if self.order_interval_ms() == 0 {
                break;
            }
        }
        self.refresh_conveyors()?;
        Ok(())
    }

    fn process_travel_due(&mut self) -> Result<(), String> {
        let arrivals = self
            .chefs
            .iter()
            .enumerate()
            .filter_map(|(index, chef)| {
                chef.travel
                    .as_ref()
                    .filter(|travel| travel.due_ms <= self.elapsed_ms)
                    .map(|travel| (index, travel.destination, travel.target.clone()))
            })
            .collect::<Vec<_>>();
        for (index, destination, target) in arrivals {
            self.chefs[index].travel = None;
            let occupied = self.chefs.iter().enumerate().any(|(other, chef)| {
                other != index && chef.respawn_due_ms.is_none() && chef.cell == destination
            });
            if self.cell_blocked(destination) || occupied {
                self.event(
                    "travel_blocked",
                    format!(
                        "Chef {} could not finish travelling to {target}.",
                        self.chefs[index].id
                    ),
                );
                continue;
            }
            self.chefs[index].cell = destination;
            self.event(
                "travel_completed",
                format!("Chef {} arrived at {target}.", self.chefs[index].id),
            );
        }
        Ok(())
    }

    fn next_recipe(&mut self) -> Option<RecipeEntry> {
        let config = &self.data.variant.config;
        if config.kind == "BossCampaignLevelConfig" {
            if !self.boss_ready {
                return None;
            }
            let entry = config
                .phases
                .get(self.boss_phase)?
                .get(self.boss_phase_index)?
                .clone();
            self.boss_phase_index += 1;
            return Some(entry);
        }
        let scripted = config.scripted_entries();
        if let Some(entry) = scripted.get(self.issued_orders) {
            return Some(entry.clone());
        }
        let recipes = config.recipe_entries();
        if recipes.is_empty() {
            return None;
        }
        if self.recipe_counts.len() != recipes.len() {
            self.recipe_counts = vec![0; recipes.len()];
        }
        let total: u32 = self.recipe_counts.iter().sum();
        let baseline = (f64::from(total) + 2.0) / recipes.len() as f64;
        let weights = self
            .recipe_counts
            .iter()
            .map(|count| (baseline - f64::from(*count)).max(0.0))
            .collect::<Vec<_>>();
        let sum: f64 = weights.iter().sum();
        let mut sample = self.random_fraction() * sum;
        let mut selected = weights.len() - 1;
        for (index, weight) in weights.iter().enumerate() {
            if sample < *weight {
                selected = index;
                break;
            }
            sample -= *weight;
        }
        self.recipe_counts[selected] += 1;
        Some(recipes[selected].clone())
    }

    fn random_fraction(&mut self) -> f64 {
        self.rng = self
            .rng
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1);
        ((self.rng >> 11) as f64) / ((1_u64 << 53) as f64)
    }

    fn random_hazard_fraction(&mut self) -> f64 {
        self.hazard_rng = self
            .hazard_rng
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1);
        ((self.hazard_rng >> 11) as f64) / ((1_u64 << 53) as f64)
    }

    fn movement_neighbor(&self, current: usize, direction: Direction) -> Option<usize> {
        let cell = &self.data.layout.walkable[current];
        let (dx, dz) = direction.vector();
        let exact = (
            cell.grid_manager.clone(),
            cell.x + dx as i32,
            cell.y,
            cell.z + dz as i32,
        );
        if let Some(index) = self.walkable_lookup.get(&exact) {
            return (!self.cell_blocked(*index)).then_some(*index);
        }
        let current_world = self.cell_world(current);
        self.data
            .layout
            .walkable
            .iter()
            .enumerate()
            .filter(|(index, _)| *index != current)
            .filter(|(index, _)| !self.cell_blocked(*index))
            .filter_map(|(index, _candidate)| {
                let candidate_world = self.cell_world(index);
                let x = candidate_world.x - current_world.x;
                let z = candidate_world.z - current_world.z;
                let vertical = (candidate_world.y - current_world.y).abs();
                let along = x * dx + z * dz;
                let sideways = (x * dz - z * dx).abs();
                (along > 0.4 && along <= 1.5 && sideways < 0.45 && vertical < 0.65)
                    .then_some((index, along))
            })
            .min_by(|left, right| left.1.total_cmp(&right.1))
            .map(|(index, _)| index)
    }

    fn destinations(&self) -> Vec<DestinationView> {
        if self.chefs[self.active_chef].respawn_due_ms.is_some()
            || self.chefs[self.active_chef].travel.is_some()
        {
            return Vec::new();
        }
        let mut destinations = self
            .data
            .layout
            .objects
            .iter()
            .filter(|object| !matches!(object_kind(object), "structure" | "moving_barrier"))
            .filter_map(|object| {
                let (destination, steps) = self.route_near(self.active_chef, &object.id).ok()??;
                Some(DestinationView {
                    target: object.id.clone(),
                    name: object.name.clone(),
                    kind: object_kind(object),
                    position: object.grid,
                    stand_position: cell_coordinate(&self.data.layout.walkable[destination]),
                    steps,
                    travel_ms: (steps as u64).saturating_mul(TRAVEL_MS_PER_CELL),
                    arrival_ms: self
                        .elapsed_ms
                        .saturating_add((steps as u64).saturating_mul(TRAVEL_MS_PER_CELL)),
                })
            })
            .chain(self.loose_items.iter().filter_map(|(id, loose)| {
                let (destination, steps) = self.route_near(self.active_chef, id).ok()??;
                Some(DestinationView {
                    target: id.clone(),
                    name: loose.item.name.clone(),
                    kind: "loose_item",
                    position: loose.grid,
                    stand_position: cell_coordinate(&self.data.layout.walkable[destination]),
                    steps,
                    travel_ms: (steps as u64).saturating_mul(TRAVEL_MS_PER_CELL),
                    arrival_ms: self
                        .elapsed_ms
                        .saturating_add((steps as u64).saturating_mul(TRAVEL_MS_PER_CELL)),
                })
            }))
            .collect::<Vec<_>>();
        destinations.sort_by(|left, right| {
            left.travel_ms
                .cmp(&right.travel_ms)
                .then_with(|| left.target.cmp(&right.target))
        });
        destinations
    }

    fn route_near(
        &self,
        chef_index: usize,
        target: &str,
    ) -> Result<Option<(usize, usize)>, String> {
        let target_world = if let Some(loose) = self.loose_items.get(target) {
            loose.world
        } else {
            self.object_world(self.object(target)?)
        };
        let start = self.chefs[chef_index].cell;
        let mut queue = VecDeque::from([(start, 0_usize)]);
        let mut visited = HashSet::from([start]);
        while let Some((current, steps)) = queue.pop_front() {
            if distance(self.cell_world(current), target_world) <= INTERACTION_DISTANCE {
                return Ok(Some((current, steps)));
            }
            for direction in [
                Direction::North,
                Direction::South,
                Direction::East,
                Direction::West,
            ] {
                if self.is_fall_edge(current, direction) {
                    continue;
                }
                let Some(next) = self.movement_neighbor(current, direction) else {
                    continue;
                };
                if self.chefs.iter().enumerate().any(|(index, chef)| {
                    index != chef_index && chef.respawn_due_ms.is_none() && chef.cell == next
                }) || !visited.insert(next)
                {
                    continue;
                }
                queue.push_back((next, steps + 1));
            }
        }
        Ok(None)
    }

    fn is_fall_edge(&self, current: usize, direction: Direction) -> bool {
        let cell = &self.data.layout.walkable[current];
        if cell.motion.is_some() {
            return true;
        }
        let (dx, dz) = direction.vector();
        self.data.layout.fall_edges.iter().any(|edge| {
            edge.grid_manager == cell.grid_manager
                && edge.from.x == cell.x
                && edge.from.y == cell.y
                && edge.from.z == cell.z
                && edge.dx == dx as i32
                && edge.dz == dz as i32
        })
    }

    fn cell_blocked(&self, index: usize) -> bool {
        let world = self.cell_world(index);
        self.data.layout.objects.iter().any(|object| {
            object.motion.is_some()
                && (matches!(object.layer, 10 | 11 | 20 | 22 | 23 | 26)
                    || object.has("AnimatedBarrier"))
                && {
                    let object_world = self.object_world(object);
                    (object_world.x - world.x).hypot(object_world.z - world.z) < 0.58
                        && (object_world.y - world.y).abs() < 0.7
                }
        })
    }

    fn object(&self, id: &str) -> Result<&GridObject, String> {
        self.data
            .layout
            .objects
            .iter()
            .find(|object| object.id == id)
            .ok_or_else(|| format!("unknown kitchen object {id:?}"))
    }

    fn require_near(&self, object: &GridObject) -> Result<(), String> {
        let chef = self.active_chef();
        if distance(self.cell_world(chef.cell), self.object_world(object)) > INTERACTION_DISTANCE {
            return Err(format!(
                "chef {} is too far from {object_id}",
                chef.id,
                object_id = object.id
            ));
        }
        Ok(())
    }

    fn require_running(&self) -> Result<(), String> {
        if !self.started {
            return Err("start the kitchen first".to_owned());
        }
        if self.boss_complete || self.elapsed_ms >= self.duration_ms() {
            return Err("the kitchen shift is complete".to_owned());
        }
        if self.data.layout.boss_flow.is_some() && !self.boss_ready {
            return Err("the boss kitchen is changing platforms".to_owned());
        }
        Ok(())
    }

    fn require_active_chef(&self) -> Result<(), String> {
        if let Some(due_ms) = self.active_chef().respawn_due_ms {
            return Err(format!(
                "chef {} is respawning for {} more milliseconds",
                self.active_chef().id,
                due_ms.saturating_sub(self.elapsed_ms)
            ));
        }
        Ok(())
    }

    fn require_idle_travel(&self) -> Result<(), String> {
        if let Some(travel) = &self.active_chef().travel {
            return Err(format!(
                "chef {} is travelling to {} for {} more milliseconds",
                self.active_chef().id,
                travel.target,
                travel.due_ms.saturating_sub(self.elapsed_ms)
            ));
        }
        Ok(())
    }

    fn active_chef(&self) -> &Chef {
        &self.chefs[self.active_chef]
    }

    fn active_chef_mut(&mut self) -> &mut Chef {
        &mut self.chefs[self.active_chef]
    }

    fn scale(&self, source_ms: u64) -> u64 {
        source_ms.saturating_mul(u64::from(self.config.time_scale))
    }

    fn scale_held_input(&self, source_ms: u64) -> u64 {
        source_ms.saturating_mul(u64::from(
            self.config.time_scale.min(MAX_HELD_INPUT_TIME_SCALE),
        ))
    }

    fn order_lifetime_ms(&self) -> u64 {
        self.scale(seconds_to_ms(
            self.data
                .variant
                .config
                .order_lifetime_seconds
                .unwrap_or(300.0),
        ))
    }

    fn order_interval_ms(&self) -> u64 {
        self.scale(seconds_to_ms(
            self.data
                .variant
                .config
                .seconds_between_orders
                .unwrap_or(15.0),
        ))
    }

    fn stars(&self) -> u8 {
        if self.data.layout.boss_flow.is_some() {
            return if self.boss_complete { 3 } else { 0 };
        }
        self.data
            .variant
            .score_star_boundaries
            .iter()
            .filter(|boundary| self.score >= **boundary)
            .count() as u8
    }

    fn item(&mut self, name: String, body: ItemBody) -> Item {
        let id = format!("item-{}", self.next_item);
        self.next_item += 1;
        Item { id, name, body }
    }

    fn object_view(&self, object: &GridObject) -> ObjectView {
        let stack = self.stacks.get(&object.id);
        ObjectView {
            id: object.id.clone(),
            name: object.name.clone(),
            kind: object_kind(object),
            grid_manager: object.grid_manager.clone(),
            position: object.grid,
            world: self.object_world(object),
            item: self.slots.get(&object.id).cloned(),
            supply: object
                .feature("PickupItemSpawner")
                .and_then(|feature| feature.get("ingredient"))
                .and_then(Value::as_str)
                .map(str::to_owned),
            processes_to: object
                .feature("PickupItemSpawner")
                .and_then(|feature| feature.get("processed_item"))
                .and_then(Value::as_str)
                .map(str::to_owned),
            plate_count: stack.filter(|stack| stack.clean).map(|stack| stack.count),
            dirty_plate_count: stack
                .filter(|stack| !stack.clean)
                .map(|stack| stack.count)
                .or_else(|| self.sinks.get(&object.id).map(|sink| sink.count)),
            enabled: self.switch_enabled.get(&object.id).copied(),
            fire_strength: self.fires.get(&object.id).map(|fire| fire.strength),
        }
    }

    fn recipe_requirements(&self, recipe: &str) -> Vec<RecipeRequirementView> {
        fn collect(
            id: &str,
            nodes: &[crate::campaign::OrderNode],
            visiting: &mut HashSet<String>,
            quantities: &mut HashMap<String, u32>,
        ) {
            if !visiting.insert(id.to_owned()) {
                return;
            }
            if let Some(node) = nodes.iter().find(|node| node.id == id) {
                for child in &node.required {
                    *quantities.entry(child.clone()).or_default() += 1;
                    collect(child, nodes, visiting, quantities);
                }
            }
            visiting.remove(id);
        }

        let mut quantities = HashMap::new();
        collect(
            recipe,
            &self.data.campaign.orders,
            &mut HashSet::new(),
            &mut quantities,
        );
        let mut requirements = quantities
            .into_iter()
            .filter_map(|(id, quantity)| {
                let node = self
                    .data
                    .campaign
                    .orders
                    .iter()
                    .find(|node| node.id == id)?;
                Some(RecipeRequirementView {
                    id: node.id.clone(),
                    quantity,
                    kind: node.kind.clone(),
                    required: node.required.clone(),
                    cooking_step: node.cooking_step.clone(),
                })
            })
            .collect::<Vec<_>>();
        requirements.sort_by(|left, right| left.id.cmp(&right.id));
        requirements
    }

    fn work_remaining_ms(&self, work: &Work) -> Option<u64> {
        match work {
            Work::Chop { target, .. } => self.slots.get(target).and_then(|item| match &item.body {
                ItemBody::Food {
                    process: Some(process),
                    work_progress_ms,
                    ..
                } => Some(process.required_ms.saturating_sub(*work_progress_ms)),
                _ => None,
            }),
            Work::Wash { target, .. } => self
                .sinks
                .get(target)
                .map(|sink| sink.clean_ms.saturating_sub(sink.progress_ms)),
            Work::Extinguish { chef, target } => self.fires.get(target).and_then(|fire| {
                self.chefs[*chef]
                    .held
                    .as_ref()
                    .and_then(|item| match item.body {
                        ItemBody::Extinguisher { extinguish_ms, .. } => {
                            Some((fire.strength * extinguish_ms as f64).ceil() as u64)
                        }
                        _ => None,
                    })
            }),
        }
    }

    fn work_views(&self) -> Vec<WorkView> {
        self.works
            .iter()
            .flatten()
            .filter_map(|work| self.work_view(work))
            .collect()
    }

    fn work_view(&self, work: &Work) -> Option<WorkView> {
        match work {
            Work::Chop { chef, target } => {
                let ItemBody::Food {
                    process: Some(process),
                    work_progress_ms,
                    ..
                } = &self.slots.get(target)?.body
                else {
                    return None;
                };
                Some(WorkView {
                    chef: self.chefs[*chef].id,
                    target: target.clone(),
                    kind: "process",
                    progress_ms: *work_progress_ms,
                    required_ms: process.required_ms,
                })
            }
            Work::Wash { chef, target } => {
                let sink = self.sinks.get(target)?;
                Some(WorkView {
                    chef: self.chefs[*chef].id,
                    target: target.clone(),
                    kind: "wash",
                    progress_ms: sink.progress_ms,
                    required_ms: sink.clean_ms,
                })
            }
            Work::Extinguish { chef, target } => {
                let fire = self.fires.get(target)?;
                let ItemBody::Extinguisher { extinguish_ms, .. } =
                    self.chefs[*chef].held.as_ref()?.body
                else {
                    return None;
                };
                Some(WorkView {
                    chef: self.chefs[*chef].id,
                    target: target.clone(),
                    kind: "extinguish",
                    progress_ms: ((1.0 - fire.strength) * extinguish_ms as f64) as u64,
                    required_ms: extinguish_ms,
                })
            }
        }
    }

    fn event(&mut self, kind: &'static str, message: impl Into<String>) {
        self.events.push(Event {
            elapsed_ms: self.elapsed_ms,
            kind,
            message: message.into(),
        });
        if self.events.len() > 128 {
            self.events.drain(..64);
        }
    }
}

enum CombineResult {
    Held,
    Placed,
    Neither,
}

fn firing_track_times(track: &MotionTrack) -> Vec<f64> {
    match track {
        MotionTrack::Curve { keys } => keys
            .iter()
            .filter(|key| key.time.is_finite() && key.time >= 0.0 && key.coefficients[3] > 0.5)
            .map(|key| key.time)
            .collect(),
        MotionTrack::Dense {
            begin,
            sample_rate,
            samples,
        } if *sample_rate > 0.0 => samples
            .iter()
            .enumerate()
            .filter(|(index, value)| **value > 0.5 && (*index == 0 || samples[*index - 1] <= 0.5))
            .map(|(index, _)| begin + index as f64 / sample_rate)
            .collect(),
        _ => Vec::new(),
    }
}

fn fireball_position(fireball: &Fireball, elapsed_ms: u64) -> Vector3 {
    let duration = fireball.due_ms.saturating_sub(fireball.spawned_ms).max(1);
    let fraction = elapsed_ms.saturating_sub(fireball.spawned_ms) as f64 / duration as f64;
    Vector3 {
        x: fireball.from.x + (fireball.to.x - fireball.from.x) * fraction.clamp(0.0, 1.0),
        y: fireball.from.y + (fireball.to.y - fireball.from.y) * fraction.clamp(0.0, 1.0),
        z: fireball.from.z + (fireball.to.z - fireball.from.z) * fraction.clamp(0.0, 1.0),
    }
}

fn fireball_collision_ms(fireball: &Fireball, chef: Vector3, elapsed_ms: u64) -> Option<u64> {
    const COLLISION_RADIUS: f64 = 0.9;
    let dx = fireball.to.x - fireball.from.x;
    let dz = fireball.to.z - fireball.from.z;
    let length_squared = dx * dx + dz * dz;
    if length_squared <= f64::EPSILON {
        return None;
    }
    let along =
        ((chef.x - fireball.from.x) * dx + (chef.z - fireball.from.z) * dz) / length_squared;
    if !(0.0..=1.0).contains(&along) {
        return None;
    }
    let closest_x = fireball.from.x + dx * along;
    let closest_z = fireball.from.z + dz * along;
    let perpendicular_squared = (chef.x - closest_x).powi(2) + (chef.z - closest_z).powi(2);
    let radius_squared = COLLISION_RADIUS * COLLISION_RADIUS;
    if perpendicular_squared > radius_squared {
        return None;
    }
    let length = length_squared.sqrt();
    let half_width = (radius_squared - perpendicular_squared).sqrt() / length;
    let enter = (along - half_width).clamp(0.0, 1.0);
    let exit = (along + half_width).clamp(0.0, 1.0);
    let duration = fireball.due_ms.saturating_sub(fireball.spawned_ms);
    let enter_ms = fireball.spawned_ms + (duration as f64 * enter).round() as u64;
    let exit_ms = fireball.spawned_ms + (duration as f64 * exit).round() as u64;
    (elapsed_ms <= exit_ms).then_some(enter_ms.max(elapsed_ms))
}

fn motion_value(fields: &Value) -> Option<MotionValue> {
    match fields.get("m_variableType")?.as_i64()? {
        0 => Some(MotionValue::Bool(fields.get("m_boolValue")?.as_i64()? != 0)),
        1 => Some(MotionValue::Int(fields.get("m_intValue")?.as_i64()?)),
        2 => Some(MotionValue::Float(fields.get("m_floatValue")?.as_f64()?)),
        _ => None,
    }
}

fn motion_condition(runtime: &MotionRuntime, condition: &crate::campaign::MotionCondition) -> bool {
    let boolean = runtime.triggers.contains(&condition.parameter)
        || matches!(
            runtime.values.get(&condition.parameter),
            Some(MotionValue::Bool(true))
        );
    let number = match runtime.values.get(&condition.parameter) {
        Some(MotionValue::Bool(value)) => i32::from(*value) as f64,
        Some(MotionValue::Int(value)) => *value as f64,
        Some(MotionValue::Float(value)) => *value,
        None => 0.0,
    };
    match condition.mode {
        1 => boolean,
        2 => !boolean,
        3 => number > condition.threshold,
        4 => number < condition.threshold,
        6 => (number - condition.threshold).abs() < f64::EPSILON,
        7 => (number - condition.threshold).abs() >= f64::EPSILON,
        _ => false,
    }
}

fn motion_track_value(track: &MotionTrack, time: f64) -> f64 {
    match track {
        MotionTrack::Constant { value } => *value,
        MotionTrack::Dense {
            begin,
            sample_rate,
            samples,
        } => {
            if samples.is_empty() {
                return 0.0;
            }
            let position = ((time - begin) * sample_rate).max(0.0);
            let left = position.floor() as usize;
            let right = (left + 1).min(samples.len() - 1);
            let fraction = position.fract();
            samples[left.min(samples.len() - 1)] * (1.0 - fraction) + samples[right] * fraction
        }
        MotionTrack::Curve { keys } => {
            let Some(key) = keys
                .iter()
                .rev()
                .find(|key| key.time <= time)
                .or_else(|| keys.first())
            else {
                return 0.0;
            };
            let delta = (time - key.time).max(0.0);
            let [a, b, c, d] = key.coefficients;
            ((a * delta + b) * delta + c) * delta + d
        }
    }
}

fn vector_add(left: Vector3, right: Vector3) -> Vector3 {
    Vector3 {
        x: left.x + right.x,
        y: left.y + right.y,
        z: left.z + right.z,
    }
}

fn vector_sub(left: Vector3, right: Vector3) -> Vector3 {
    Vector3 {
        x: left.x - right.x,
        y: left.y - right.y,
        z: left.z - right.z,
    }
}

fn quaternion_multiply(left: Quaternion, right: Quaternion) -> Quaternion {
    Quaternion {
        x: left.w * right.x + left.x * right.w + left.y * right.z - left.z * right.y,
        y: left.w * right.y - left.x * right.z + left.y * right.w + left.z * right.x,
        z: left.w * right.z + left.x * right.y - left.y * right.x + left.z * right.w,
        w: left.w * right.w - left.x * right.x - left.y * right.y - left.z * right.z,
    }
}

fn quaternion_inverse(value: Quaternion) -> Quaternion {
    let magnitude = value.x * value.x + value.y * value.y + value.z * value.z + value.w * value.w;
    Quaternion {
        x: -value.x / magnitude,
        y: -value.y / magnitude,
        z: -value.z / magnitude,
        w: value.w / magnitude,
    }
}

fn quaternion_normalize(value: Quaternion) -> Quaternion {
    let magnitude =
        (value.x * value.x + value.y * value.y + value.z * value.z + value.w * value.w).sqrt();
    Quaternion {
        x: value.x / magnitude,
        y: value.y / magnitude,
        z: value.z / magnitude,
        w: value.w / magnitude,
    }
}

fn quaternion_rotate_vector(rotation: Quaternion, value: Vector3) -> Vector3 {
    let vector = Quaternion {
        x: value.x,
        y: value.y,
        z: value.z,
        w: 0.0,
    };
    let rotated = quaternion_multiply(
        quaternion_multiply(rotation, vector),
        quaternion_inverse(rotation),
    );
    Vector3 {
        x: rotated.x,
        y: rotated.y,
        z: rotated.z,
    }
}

fn quaternion_from_euler(euler: [f64; 3]) -> Quaternion {
    let [x, y, z] = euler.map(|value| value.to_radians() / 2.0);
    let qx = Quaternion {
        x: x.sin(),
        y: 0.0,
        z: 0.0,
        w: x.cos(),
    };
    let qy = Quaternion {
        x: 0.0,
        y: y.sin(),
        z: 0.0,
        w: y.cos(),
    };
    let qz = Quaternion {
        x: 0.0,
        y: 0.0,
        z: z.sin(),
        w: z.cos(),
    };
    quaternion_normalize(quaternion_multiply(quaternion_multiply(qy, qx), qz))
}

fn item_from_properties(properties: FoodProperties, process: Option<ProcessSpec>) -> ItemBody {
    if let Some(capacity) = properties.container_capacity {
        ItemBody::Container {
            base_order: properties.order,
            capacity,
            contents: Vec::new(),
            cooking: properties.cooking.map(|cooking| Cooking {
                step: cooking.step,
                station_type: cooking.station_type,
                duration_ms: cooking.duration_ms,
                progress_ms: 0,
            }),
        }
    } else {
        ItemBody::Food {
            order: properties.order,
            process,
            work_progress_ms: 0,
        }
    }
}

fn food_properties(value: &Value, time_scale: u32) -> Result<FoodProperties, String> {
    let object = value
        .as_object()
        .ok_or_else(|| "food properties must be an object".to_owned())?;
    let order = object
        .get("order")
        .and_then(Value::as_str)
        .map(str::to_owned);
    let container_capacity = object
        .get("container_capacity")
        .and_then(Value::as_u64)
        .map(|value| value as usize);
    let cooking = object
        .get("cooking")
        .and_then(Value::as_object)
        .map(|cooking| {
            Ok::<CookingSpec, String>(CookingSpec {
                step: string(cooking, "step")?,
                station_type: cooking
                    .get("station_type")
                    .and_then(Value::as_i64)
                    .ok_or_else(|| "cooking station type is missing".to_owned())?,
                duration_ms: seconds_to_ms(number(cooking, "seconds")?)
                    .saturating_mul(u64::from(time_scale)),
            })
        })
        .transpose()?;
    Ok(FoodProperties {
        order,
        container_capacity,
        cooking,
    })
}

fn add_to_container(
    contents: &mut Vec<String>,
    capacity: usize,
    order: String,
    cooking: &mut Option<Cooking>,
) -> Result<(), String> {
    if contents.len() >= capacity {
        return Err("the container is full".to_owned());
    }
    contents.push(order);
    if let Some(cooking) = cooking {
        cooking.progress_ms = 0;
    }
    Ok(())
}

fn clear_if_utensil(item: &mut Item) -> bool {
    let ItemBody::Container {
        base_order,
        contents,
        cooking,
        ..
    } = &mut item.body
    else {
        return false;
    };
    if base_order.is_some() {
        return false;
    }
    contents.clear();
    if let Some(cooking) = cooking {
        cooking.progress_ms = 0;
    }
    true
}

fn same_multiset(left: &[String], right: &[String]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    let mut left = left.to_vec();
    let mut right = right.to_vec();
    left.sort();
    right.sort();
    left == right
}

fn nearest_cell(cells: &[GridCell], world: Vector3) -> Option<usize> {
    cells
        .iter()
        .enumerate()
        .min_by(|(_, left), (_, right)| {
            distance(left.world, world).total_cmp(&distance(right.world, world))
        })
        .map(|(index, _)| index)
}

fn distance(left: Vector3, right: Vector3) -> f64 {
    ((left.x - right.x).powi(2) + (left.y - right.y).powi(2) + (left.z - right.z).powi(2)).sqrt()
}

fn cell_coordinate(cell: &GridCell) -> Coordinate {
    Coordinate {
        x: cell.x,
        y: cell.y,
        z: cell.z,
    }
}

fn object_kind(object: &GridObject) -> &'static str {
    let kind = [
        ("PickupItemSpawner", "ingredient_crate"),
        ("Workstation", "workstation"),
        ("CookingStation", "cooker"),
        ("ConveyorStation", "conveyor"),
        ("PlateStation", "delivery"),
        ("WashingStation", "sink"),
        ("PlateReturnStation", "plate_return"),
        ("RubbishBin", "bin"),
        ("AttachStation", "counter"),
        ("Interactable", "switch"),
    ]
    .into_iter()
    .find(|(component, _)| object.has(component))
    .map(|(_, kind)| kind);
    kind.unwrap_or_else(|| {
        if object.has("AnimatedBarrier")
            || (object.dynamic && matches!(object.layer, 10 | 11 | 20 | 22 | 23 | 26))
        {
            "moving_barrier"
        } else {
            "structure"
        }
    })
}

fn string(object: &serde_json::Map<String, Value>, key: &str) -> Result<String, String> {
    object
        .get(key)
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| format!("{key} must be a string"))
}

fn number(object: &serde_json::Map<String, Value>, key: &str) -> Result<f64, String> {
    object
        .get(key)
        .and_then(Value::as_f64)
        .ok_or_else(|| format!("{key} must be a number"))
}

#[cfg(test)]
mod tests {
    use std::collections::{HashMap, HashSet, VecDeque};
    use std::path::Path;

    use super::*;
    use crate::campaign::GameData;

    fn data(level: u8) -> GameData {
        GameData::load(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("data/overcooked-1"),
            level,
        )
        .expect("imported game data")
    }

    fn walk_near(session: &mut Session<'_>, target: &str) {
        let target_world = session.object(target).expect("target").world;
        walk_until(session, |world| {
            distance(world, target_world) <= INTERACTION_DISTANCE
        });
    }

    fn walk_until(session: &mut Session<'_>, reached: impl Fn(Vector3) -> bool) {
        let start = session.active_chef().cell;
        let blocked = session.chefs[1 - session.active_chef].cell;
        let mut queue = VecDeque::from([start]);
        let mut previous = HashMap::<usize, (usize, Direction)>::new();
        let mut visited = HashSet::from([start]);
        let destination = loop {
            let current = queue.pop_front().expect("target is reachable");
            if reached(session.cell_world(current)) {
                break current;
            }
            for direction in [
                Direction::North,
                Direction::South,
                Direction::East,
                Direction::West,
            ] {
                if let Some(next) = session.movement_neighbor(current, direction)
                    && next != blocked
                    && visited.insert(next)
                {
                    previous.insert(next, (current, direction));
                    queue.push_back(next);
                }
            }
        };
        let mut path = Vec::new();
        let mut cursor = destination;
        while cursor != start {
            let (prior, direction) = previous[&cursor];
            path.push(direction);
            cursor = prior;
        }
        for direction in path.into_iter().rev() {
            session.move_chef(direction, false).expect("walk");
        }
    }

    fn path_near(session: &Session<'_>, target: &str) -> Option<Vec<Direction>> {
        let target_world = session.object_world(session.object(target).expect("target object"));
        let start = session.active_chef().cell;
        let blocked = session.chefs[1 - session.active_chef].cell;
        let mut queue = VecDeque::from([start]);
        let mut previous = HashMap::<usize, (usize, Direction)>::new();
        let mut visited = HashSet::from([start]);
        let mut destination = None;
        while let Some(current) = queue.pop_front() {
            if distance(session.cell_world(current), target_world) <= INTERACTION_DISTANCE {
                destination = Some(current);
                break;
            }
            for direction in [
                Direction::North,
                Direction::South,
                Direction::East,
                Direction::West,
            ] {
                if let Some(next) = session.movement_neighbor(current, direction)
                    && next != blocked
                    && visited.insert(next)
                {
                    previous.insert(next, (current, direction));
                    queue.push_back(next);
                }
            }
        }
        let mut cursor = destination?;
        let mut path = Vec::new();
        while cursor != start {
            let (prior, direction) = previous[&cursor];
            path.push(direction);
            cursor = prior;
        }
        path.reverse();
        Some(path)
    }

    fn walk_near_eventually(session: &mut Session<'_>, target: &str) {
        let deadline = session.duration_ms();
        while session.elapsed_ms < deadline {
            if let Some(path) = path_near(session, target) {
                for direction in path {
                    session.move_chef(direction, false).expect("walk");
                }
                return;
            }
            session
                .advance_to((session.elapsed_ms + 500).min(deadline))
                .expect("wait for authored moving grids");
        }
        panic!("target {target} never became reachable");
    }

    fn walk_near_any_eventually(session: &mut Session<'_>, targets: &[String]) -> String {
        let deadline = session.duration_ms();
        while session.elapsed_ms < deadline {
            for target in targets {
                if let Some(path) = path_near(session, target) {
                    for direction in path {
                        session.move_chef(direction, false).expect("walk");
                    }
                    return target.clone();
                }
            }
            session
                .advance_to((session.elapsed_ms + 500).min(deadline))
                .expect("wait for an authored moving grid");
        }
        panic!("no candidate target ever became reachable");
    }

    fn empty_counter_near(session: &Session<'_>, target: &str) -> String {
        let target = session.object(target).expect("target station");
        session
            .data
            .layout
            .objects
            .iter()
            .filter(|object| object_kind(object) == "counter")
            .filter(|object| object.grid_manager == target.grid_manager)
            .filter(|object| !session.slots.contains_key(&object.id))
            .min_by(|left, right| {
                distance(session.object_world(left), session.object_world(target)).total_cmp(
                    &distance(session.object_world(right), session.object_world(target)),
                )
            })
            .expect("empty counter beside station")
            .id
            .clone()
    }

    fn take_authored_extinguisher(session: &mut Session<'_>) {
        let id = session
            .loose_items
            .iter()
            .find_map(|(id, loose)| {
                matches!(loose.item.body, ItemBody::Extinguisher { .. }).then(|| id.clone())
            })
            .expect("authored extinguisher");
        let world = session.loose_items[&id].world;
        walk_until(session, |cell| {
            distance(cell, world) <= INTERACTION_DISTANCE
        });
        session.interact(&id).expect("take extinguisher");
    }

    fn fight_fires_until(session: &mut Session<'_>, deadline: u64) {
        while session.elapsed_ms < deadline {
            if session.works[session.active_chef].is_some() {
                session.stop_work().expect("release extinguisher");
            }
            let reachable = {
                let mut fires = session.fires.keys().cloned().collect::<Vec<_>>();
                fires.sort();
                fires
                    .into_iter()
                    .find_map(|target| path_near(session, &target).map(|path| (target, path)))
            };
            let Some((target, path)) = reachable else {
                session
                    .advance_to((session.elapsed_ms + 500).min(deadline))
                    .expect("watch for hazards");
                continue;
            };
            for direction in path {
                session.move_chef(direction, false).expect("reach fire");
            }
            let due = session
                .start_work(&target)
                .expect("hold extinguisher input");
            session
                .advance_to(due.min(deadline))
                .expect("spray active fire");
            if due > deadline && session.works[session.active_chef].is_some() {
                session.stop_work().expect("release extinguisher");
            }
        }
    }

    fn clear_active_fires(session: &mut Session<'_>) {
        while !session.fires.is_empty() {
            let deadline = (session.elapsed_ms + 500).min(session.duration_ms());
            fight_fires_until(session, deadline);
            assert!(
                session.elapsed_ms < session.duration_ms(),
                "shift ended while fighting fire"
            );
        }
    }

    fn prepare_on_board(session: &mut Session<'_>, crate_id: &str, board_id: &str) {
        walk_near(session, crate_id);
        session.interact(crate_id).expect("take raw ingredient");
        walk_near(session, board_id);
        session.interact(board_id).expect("put ingredient on board");
        let finished_ms = session.start_work(board_id).expect("hold chop input");
        session
            .advance_to(finished_ms)
            .expect("real time reaches the final chop");
        session
            .interact(board_id)
            .expect("take processed ingredient");
    }

    fn authored_plate_slot(session: &Session<'_>) -> String {
        session
            .slots
            .iter()
            .find_map(|(id, item)| matches!(item.body, ItemBody::Plate { .. }).then(|| id.clone()))
            .expect("authored plate")
    }

    #[test]
    fn start_opens_the_original_first_order_and_time_expires_it() {
        let data = data(1);
        let mut session = Session::new(
            &data,
            SessionConfig {
                time_scale: 1,
                seed: 1,
            },
        )
        .expect("session");
        session.start().expect("start");
        assert_eq!(session.snapshot().orders.len(), 1);
        assert_eq!(session.snapshot().orders[0].recipe, "Salad_Tomato");
        session.advance_to(100_000).expect("advance");
        assert_eq!(session.snapshot().shift.status, "complete");
        assert_eq!(session.snapshot().campaign.score, -10);
        assert!(
            session
                .snapshot()
                .recent_events
                .iter()
                .any(|event| event.kind == "order_overdue")
        );
    }

    #[test]
    fn an_overdue_order_loses_points_and_restarts_instead_of_disappearing() {
        let data = data(3);
        let mut session = Session::new(
            &data,
            SessionConfig {
                time_scale: 1,
                seed: 1,
            },
        )
        .expect("session");
        session.start().expect("start");
        let order_id = session.orders[0].id.clone();
        let first_deadline = session.orders[0].deadline_ms;
        session
            .advance_to(first_deadline)
            .expect("order timer expires");
        assert_eq!(session.score, -10);
        let order = session
            .orders
            .iter()
            .find(|order| order.id == order_id)
            .expect("overdue order remains active");
        assert_eq!(order.deadline_ms, first_deadline * 2);
        assert!(
            session
                .snapshot()
                .recent_events
                .iter()
                .any(|event| event.kind == "order_overdue")
        );
    }

    #[test]
    fn soup_level_exposes_quantity_cooking_step_and_real_deadline_pressure() {
        let data = data(6);
        let mut session = Session::new(
            &data,
            SessionConfig {
                time_scale: 5,
                seed: 448_516,
            },
        )
        .expect("session");
        session.start().expect("start");
        let snapshot = session.snapshot();
        let order = snapshot.orders.first().expect("authored soup order");
        assert_eq!(order.recipe, "OnionSoup");
        assert_eq!(order.cooking_step.as_deref(), Some("Pot"));
        assert_eq!(order.deadline_ms, 750_000);
        assert!(order.deadline_ms < snapshot.shift.duration_ms);
        assert_eq!(
            order
                .requirements
                .iter()
                .find(|requirement| requirement.id == "Onion")
                .map(|requirement| requirement.quantity),
            Some(3)
        );
    }

    #[test]
    fn every_level_builds_and_replays_the_full_shift_deterministically() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("data/overcooked-1");
        for level in 1..=30 {
            let data = GameData::load(&root, level).expect("level");
            let mut runs = Vec::new();
            for _ in 0..2 {
                let mut session = Session::new(
                    &data,
                    SessionConfig {
                        time_scale: 1,
                        seed: 1,
                    },
                )
                .unwrap_or_else(|error| panic!("level {level}: {error}"));
                assert_eq!(session.snapshot().chefs.len(), 2);
                assert!(!session.snapshot().map.objects.is_empty());
                session.start().expect("start full level replay");
                session.final_score();
                let snapshot = session.snapshot();
                assert_eq!(snapshot.shift.status, "complete");
                runs.push(serde_json::to_value(snapshot).expect("serialize final state"));
            }
            assert_eq!(runs[0], runs[1], "level {level} replay diverged");
        }
    }

    #[test]
    fn action_surface_has_no_batch_or_future_action() {
        let controls = data(1);
        let session = Session::new(&controls, SessionConfig::default()).expect("session");
        let names = session
            .snapshot()
            .controls
            .into_iter()
            .collect::<HashSet<_>>();
        assert!(!names.contains("batch"));
        assert!(!names.contains("schedule_action"));
        assert!(names.contains("alarm"));
    }

    #[test]
    fn snapshot_exposes_authored_supplies_and_transitive_recipe_plan() {
        let data = data(21);
        let mut session = Session::new(
            &data,
            SessionConfig {
                time_scale: 1,
                seed: 1,
            },
        )
        .expect("session");
        session.start().expect("start");
        let snapshot = session.snapshot();
        assert!(
            snapshot
                .map
                .objects
                .iter()
                .any(|object| object.supply.as_deref() == Some("Tortilla"))
        );
        let order = snapshot.orders.first().expect("authored burrito order");
        let requirements = order
            .requirements
            .iter()
            .map(|requirement| requirement.id.as_str())
            .collect::<HashSet<_>>();
        assert!(requirements.contains("Tortilla"));
        assert!(requirements.contains("BoiledRice"));
        assert!(requirements.contains("Rice"));
    }

    #[test]
    fn go_exposes_and_pays_the_shortest_semantic_travel_time() {
        let data = data(1);
        let mut session = Session::new(
            &data,
            SessionConfig {
                time_scale: 1,
                seed: 1,
            },
        )
        .expect("session");
        session.start().expect("start");
        let board = session
            .snapshot()
            .destinations
            .into_iter()
            .find(|destination| destination.steps > 0)
            .expect("non-local reachable destination");
        assert_eq!(board.travel_ms, board.steps as u64 * TRAVEL_MS_PER_CELL);

        let due = session.go(&board.target).expect("start semantic travel");
        assert_eq!(due, board.arrival_ms);
        assert_eq!(session.next_attention_ms(), Some(due));
        assert!(session.interact(&board.target).is_err());
        session.advance_to(due).expect("arrive");
        let active = session
            .snapshot()
            .chefs
            .into_iter()
            .find(|chef| chef.active)
            .expect("active chef");
        assert!(active.travel.is_none());
        assert!(
            distance(
                active.world,
                session.object_world(session.object(&board.target).unwrap())
            ) <= INTERACTION_DISTANCE
        );
    }

    #[test]
    fn held_input_stretch_is_capped_below_passive_shift_scale() {
        let data = data(1);
        let mut session = Session::new(
            &data,
            SessionConfig {
                time_scale: 12,
                seed: 1,
            },
        )
        .expect("session");
        session.start().expect("start");

        let source = data
            .layout
            .objects
            .iter()
            .find(|object| {
                object
                    .feature("PickupItemSpawner")
                    .is_some_and(|feature| feature.get("processed_item").is_some())
            })
            .expect("processable ingredient supply");
        let feature = source
            .feature("PickupItemSpawner")
            .expect("ingredient feature");
        let stages = feature
            .get("work_stages")
            .and_then(Value::as_u64)
            .expect("work stages");
        let source_ms = (data.campaign.scoring.chop_impact_seconds * 1_000.0).round() as u64
            * stages.saturating_sub(1)
            * u64::from(data.campaign.scoring.single_player_chops_per_stage);
        let board = data
            .layout
            .objects
            .iter()
            .find(|object| object.has("Workstation"))
            .expect("workstation");

        walk_near(&mut session, &source.id);
        session.interact(&source.id).expect("take ingredient");
        walk_near(&mut session, &board.id);
        session.interact(&board.id).expect("put ingredient down");
        session.start_work(&board.id).expect("start chopping");

        assert_eq!(
            session.snapshot().works[0].required_ms,
            source_ms * u64::from(MAX_HELD_INPUT_TIME_SCALE)
        );
        assert_eq!(
            session.duration_ms(),
            seconds_to_ms(100.0) * u64::from(session.config.time_scale)
        );
    }

    #[test]
    fn both_chefs_can_hold_independent_work_inputs_after_switching() {
        let data = data(1);
        let mut session = Session::new(&data, SessionConfig::default()).expect("session");
        session.start().expect("start");

        walk_near(&mut session, "object-1117");
        session.interact("object-1117").expect("take lettuce");
        walk_near(&mut session, "object-1178");
        session.interact("object-1178").expect("place lettuce");
        let lettuce_done = session.start_work("object-1178").expect("chop lettuce");

        session.switch().expect("control second chef");
        assert_eq!(session.snapshot().works.len(), 1);
        walk_near(&mut session, "object-681");
        session.interact("object-681").expect("take tomato");
        walk_near(&mut session, "object-1239");
        session.interact("object-1239").expect("place tomato");
        let tomato_done = session.start_work("object-1239").expect("chop tomato");

        let snapshot = session.snapshot();
        assert_eq!(snapshot.works.len(), 2);
        assert_eq!(
            snapshot
                .works
                .iter()
                .map(|work| work.chef)
                .collect::<HashSet<_>>(),
            HashSet::from([0, 1])
        );

        session
            .advance_to(lettuce_done.max(tomato_done))
            .expect("finish both held inputs");
        assert!(session.snapshot().works.is_empty());
        assert!(
            session
                .snapshot()
                .recent_events
                .iter()
                .filter(|event| event.kind == "food_processed")
                .count()
                >= 2
        );
    }

    #[test]
    fn original_first_order_can_be_prepared_plated_and_delivered() {
        let data = data(1);
        let mut session = Session::new(
            &data,
            SessionConfig {
                time_scale: 1,
                seed: 1,
            },
        )
        .expect("session");
        session.start().expect("start");

        let plate_slot = session
            .slots
            .iter()
            .find_map(|(id, item)| matches!(item.body, ItemBody::Plate { .. }).then(|| id.clone()))
            .expect("authored initial plate");

        prepare_on_board(&mut session, "object-1117", "object-1178");
        walk_near(&mut session, &plate_slot);
        session
            .interact(&plate_slot)
            .expect("add chopped lettuce to plate");

        prepare_on_board(&mut session, "object-681", "object-1178");
        walk_near(&mut session, &plate_slot);
        session
            .interact(&plate_slot)
            .expect("add chopped tomato to plate");
        session
            .interact(&plate_slot)
            .expect("pick up completed salad");

        walk_near(&mut session, "object-733");
        session.interact("object-733").expect("deliver salad");

        let snapshot = session.snapshot();
        assert!(snapshot.campaign.score >= 20);
        assert!(snapshot.orders.iter().all(|order| order.id != "order-1"));
        assert!(
            snapshot
                .recent_events
                .iter()
                .any(|event| event.kind == "order_delivered")
        );
    }

    #[test]
    fn original_onion_soup_cooks_in_real_time_and_pouring_keeps_one_empty_pot() {
        let data = data(2);
        let mut session = Session::new(
            &data,
            SessionConfig {
                time_scale: 1,
                seed: 1,
            },
        )
        .expect("session");
        session.start().expect("start");
        let plate_slot = session
            .slots
            .iter()
            .find_map(|(id, item)| matches!(item.body, ItemBody::Plate { .. }).then(|| id.clone()))
            .expect("authored plate");

        for _ in 0..3 {
            prepare_on_board(&mut session, "object-3445", "object-4134");
            walk_near(&mut session, "object-3526");
            session
                .interact("object-3526")
                .expect("add chopped onion to pot");
        }
        let ready_ms = session.elapsed_ms
            + match &session.slots["object-3526"].body {
                ItemBody::Container {
                    cooking: Some(cooking),
                    ..
                } => cooking.duration_ms,
                _ => panic!("authored pot on cooker"),
            };
        session
            .advance_to(ready_ms - 1)
            .expect("soup is almost ready");
        walk_near(&mut session, &plate_slot);
        session.interact(&plate_slot).expect("take plate");
        walk_near(&mut session, "object-3526");
        assert!(
            session.interact("object-3526").is_err(),
            "undercooked soup cannot be poured"
        );
        session.advance_to(ready_ms).expect("soup is ready");
        session
            .interact("object-3526")
            .expect("pour soup onto plate");
        assert!(matches!(
            &session.slots["object-3526"].body,
            ItemBody::Container { contents, .. } if contents.is_empty()
        ));
        assert!(matches!(
            &session.active_chef().held,
            Some(Item {
                body: ItemBody::Plate { contents },
                ..
            }) if contents == &["OnionSoup".to_owned()]
        ));

        walk_near(&mut session, "object-3513");
        session
            .interact("object-3513")
            .expect("deliver original first soup");
        assert!(
            session
                .snapshot()
                .recent_events
                .iter()
                .any(|event| event.kind == "order_delivered")
        );
        let returned_ms =
            session.elapsed_ms + seconds_to_ms(session.data.variant.config.plate_return_seconds);
        session
            .advance_to(returned_ms)
            .expect("dirty plate returns");
        assert_eq!(session.stacks["object-4986"].count, 1);
        walk_near(&mut session, "object-4986");
        session
            .interact("object-4986")
            .expect("take dirty plate stack");
        walk_near(&mut session, "object-3855");
        session
            .interact("object-3855")
            .expect("put dirty plates in sink");
        let washed_ms = session.start_work("object-3855").expect("hold wash input");
        session.advance_to(washed_ms).expect("plate is washed");
        assert_eq!(session.stacks["object-3854"].count, 1);
    }

    #[test]
    fn original_beef_burger_cooks_in_a_pan_and_requires_a_plate() {
        let data = data(5);
        let mut session = Session::new(
            &data,
            SessionConfig {
                time_scale: 1,
                seed: 0,
            },
        )
        .expect("session");
        session.start().expect("start");
        let plate_slot = authored_plate_slot(&session);

        prepare_on_board(&mut session, "object-4077", "object-4909");
        walk_near(&mut session, "object-3691");
        session
            .interact("object-3691")
            .expect("put chopped meat in pan");
        let ready_ms = session.elapsed_ms
            + match &session.slots["object-3691"].body {
                ItemBody::Container {
                    cooking: Some(cooking),
                    ..
                } => cooking.duration_ms,
                _ => panic!("authored pan on cooker"),
            };

        walk_near(&mut session, "object-3989");
        session.interact("object-3989").expect("take bun");
        walk_near(&mut session, "object-4231");
        session.interact("object-4231").expect("put bun down");
        session.advance_to(ready_ms).expect("meat fries");
        walk_near(&mut session, "object-3691");
        session.interact("object-3691").expect("take cooked pan");
        walk_near(&mut session, "object-4231");
        session
            .interact("object-4231")
            .expect("put fried meat in bun");
        assert!(matches!(
            &session.active_chef().held,
            Some(Item {
                body: ItemBody::Container { contents, .. },
                ..
            }) if contents.is_empty()
        ));
        walk_near(&mut session, "object-3691");
        session.interact("object-3691").expect("return empty pan");
        walk_near(&mut session, "object-4231");
        session.interact("object-4231").expect("take burger");
        walk_near(&mut session, &plate_slot);
        session.interact(&plate_slot).expect("put burger on plate");
        session.interact(&plate_slot).expect("take plated burger");
        walk_near(&mut session, "object-4448");
        session
            .interact("object-4448")
            .expect("deliver beef burger");

        assert!(
            session
                .snapshot()
                .recent_events
                .iter()
                .any(|event| event.kind == "order_delivered")
        );
    }

    #[test]
    fn frozen_hazard_kitchen_can_serve_its_first_burrito_across_moving_islands() {
        let data = data(21);
        let mut session = Session::new(
            &data,
            SessionConfig {
                time_scale: 5,
                seed: 448_510,
            },
        )
        .expect("session");
        session.start().expect("start");
        assert_eq!(
            session
                .snapshot()
                .map
                .walkable
                .iter()
                .filter(|cell| cell.moving)
                .count(),
            38
        );
        session.switch().expect("control chef on the supply island");

        let supply = |session: &Session<'_>, ingredient: &str| {
            session
                .data
                .layout
                .objects
                .iter()
                .find(|object| {
                    object
                        .feature("PickupItemSpawner")
                        .and_then(|feature| feature.get("ingredient"))
                        .and_then(Value::as_str)
                        == Some(ingredient)
                })
                .expect("authored ingredient supply")
                .id
                .clone()
        };
        let cooker = |session: &Session<'_>, step: &str| {
            session
                .slots
                .iter()
                .find_map(|(id, item)| {
                    matches!(
                        &item.body,
                        ItemBody::Container {
                            cooking: Some(cooking),
                            ..
                        } if cooking.step == step
                    )
                    .then(|| id.clone())
                })
                .expect("authored cooking vessel")
        };
        let rice = supply(&session, "Rice");
        let chicken = supply(&session, "Chicken");
        let tortilla = supply(&session, "Tortilla");
        let pot = cooker(&session, "Pot");
        let pan = cooker(&session, "FryingPan");
        let board = session
            .data
            .layout
            .objects
            .iter()
            .find(|object| object.has("Workstation"))
            .expect("authored chopping board")
            .id
            .clone();
        let pan_counter = empty_counter_near(&session, &pan);
        let pot_counter = pan_counter.clone();
        let plates = session
            .slots
            .iter()
            .filter(|(_, item)| matches!(item.body, ItemBody::Plate { .. }))
            .map(|(id, _)| id.clone())
            .collect::<Vec<_>>();
        let delivery = session
            .data
            .layout
            .objects
            .iter()
            .find(|object| object.has("PlateStation"))
            .expect("authored delivery")
            .id
            .clone();

        let plate = walk_near_any_eventually(&mut session, &plates);
        session.interact(&plate).expect("take a plate");
        walk_near_eventually(&mut session, &pot_counter);
        session.interact(&pot_counter).expect("stage plate by pot");
        walk_near_eventually(&mut session, &rice);
        session.interact(&rice).expect("take rice");
        walk_near_eventually(&mut session, &pot);
        session.interact(&pot).expect("put rice in pot");
        let rice_ready = match &session.slots[&pot].body {
            ItemBody::Container {
                cooking: Some(cooking),
                ..
            } => session.elapsed_ms + cooking.duration_ms.saturating_sub(cooking.progress_ms),
            _ => panic!("rice cooks in authored pot"),
        };
        session.switch().expect("control firefighter");
        take_authored_extinguisher(&mut session);
        fight_fires_until(&mut session, rice_ready);
        clear_active_fires(&mut session);
        session.switch().expect("return to cook");
        walk_near_eventually(&mut session, &pot_counter);
        session.interact(&pot_counter).expect("take staged plate");
        walk_near_eventually(&mut session, &pot);
        session.interact(&pot).expect("pour rice onto plate");
        walk_near_eventually(&mut session, &pot_counter);
        session.interact(&pot_counter).expect("stage rice plate");
        walk_near_eventually(&mut session, &tortilla);
        session.interact(&tortilla).expect("take tortilla");
        walk_near_eventually(&mut session, &pot_counter);
        session
            .interact(&pot_counter)
            .expect("add tortilla to plate");
        session
            .interact(&pot_counter)
            .expect("take partial burrito");

        walk_near_eventually(&mut session, &pan_counter);
        session.interact(&pan_counter).expect("stage plate by pan");
        walk_near_eventually(&mut session, &chicken);
        session.interact(&chicken).expect("take chicken");
        session.switch().expect("clear the chopping board");
        clear_active_fires(&mut session);
        session.switch().expect("return to raw chicken");
        walk_near_eventually(&mut session, &board);
        session.interact(&board).expect("put chicken on board");
        let chopped = session.start_work(&board).expect("hold chop input");
        session.advance_to(chopped).expect("chop chicken");
        session.switch().expect("clear fires after chopping");
        clear_active_fires(&mut session);
        session.switch().expect("return to chopped chicken");
        session.interact(&board).expect("take chopped chicken");
        walk_near_eventually(&mut session, &pan);
        session.interact(&pan).expect("put chicken in pan");
        walk_near_eventually(&mut session, &pan_counter);
        session.interact(&pan_counter).expect("take staged plate");
        let chicken_ready = match &session.slots[&pan].body {
            ItemBody::Container {
                cooking: Some(cooking),
                ..
            } => session.elapsed_ms + cooking.duration_ms.saturating_sub(cooking.progress_ms),
            _ => panic!("chicken fries in authored pan"),
        };
        session.switch().expect("fight fires while chicken fries");
        fight_fires_until(&mut session, chicken_ready);
        clear_active_fires(&mut session);
        session.switch().expect("return to chicken");
        walk_near_eventually(&mut session, &pan);
        session.interact(&pan).expect("add chicken to plate");
        walk_near_eventually(&mut session, &delivery);
        session.interact(&delivery).expect("serve chicken burrito");

        assert!(session.score >= 20);
        assert!(session.elapsed_ms < session.duration_ms());
    }

    #[test]
    fn original_margherita_is_assembled_on_dough_then_cooked_in_an_oven() {
        let data = data(16);
        let mut session = Session::new(
            &data,
            SessionConfig {
                time_scale: 1,
                seed: 0,
            },
        )
        .expect("session");
        session.start().expect("start");
        let plate_slot = authored_plate_slot(&session);
        let assembly_counter = "object-1836";

        prepare_on_board(&mut session, "object-2104", "object-2951");
        walk_near(&mut session, assembly_counter);
        session
            .interact(assembly_counter)
            .expect("put processed dough down");
        for crate_id in ["object-1989", "object-1832"] {
            prepare_on_board(&mut session, crate_id, "object-2951");
            walk_near(&mut session, assembly_counter);
            session
                .interact(assembly_counter)
                .expect("add pizza topping");
        }
        session
            .interact(assembly_counter)
            .expect("take uncooked pizza");
        walk_near(&mut session, "object-1904");
        session.interact("object-1904").expect("put pizza in oven");
        let ready_ms = session.elapsed_ms
            + match &session.slots["object-1904"].body {
                ItemBody::Container {
                    cooking: Some(cooking),
                    ..
                } => cooking.duration_ms,
                _ => panic!("pizza cooks as its own authored container"),
            };
        session.advance_to(ready_ms).expect("pizza bakes");
        session.interact("object-1904").expect("take cooked pizza");
        walk_near(&mut session, &plate_slot);
        session
            .interact(&plate_slot)
            .expect("put cooked pizza on plate");
        session.interact(&plate_slot).expect("take plated pizza");
        walk_near(&mut session, "object-1926");
        session.interact("object-1926").expect("deliver margherita");

        assert!(
            session
                .snapshot()
                .recent_events
                .iter()
                .any(|event| event.kind == "order_delivered")
        );
    }

    #[test]
    fn authored_shuttle_switch_drives_the_imported_state_machine_and_motion_curve() {
        let data = data(20);
        let mut session = Session::new(
            &data,
            SessionConfig {
                time_scale: 1,
                seed: 1,
            },
        )
        .expect("session");
        session.start().expect("start");
        assert!(session.switch_enabled["object-2086"]);
        assert!(!session.switch_enabled["object-2005"]);
        let moving_cell = session
            .data
            .layout
            .walkable
            .iter()
            .position(|cell| cell.grid_manager == "grid-2170")
            .expect("shuttle floor");
        let initial = session.cell_world(moving_cell);
        let initial_rice = session.object_world(
            session
                .object("object-1988")
                .expect("authored shuttle rice supply"),
        );

        walk_near(&mut session, "object-2086");
        session.interact("object-2086").expect("press west switch");
        assert!(!session.switch_enabled["object-2086"]);
        assert!(session.interact("object-2086").is_err());

        session.advance_to(4_000).expect("shuttle is travelling");
        let moving = session.cell_world(moving_cell);
        assert!(distance(initial, moving) > 1.0);
        assert!(
            distance(
                initial_rice,
                session.object_world(
                    session
                        .object("object-1988")
                        .expect("authored shuttle rice supply"),
                ),
            ) > 1.0
        );

        session.advance_to(8_000).expect("shuttle reaches east");
        assert!(session.switch_enabled["object-2005"]);
        assert!(!session.switch_enabled["object-2086"]);
    }

    #[test]
    fn authored_conveyor_uses_two_phase_real_time_transfer_and_chains() {
        let data = data(27);
        let mut session = Session::new(
            &data,
            SessionConfig {
                time_scale: 1,
                seed: 1,
            },
        )
        .expect("session");
        session.start().expect("start");
        let conveyor = session
            .data
            .layout
            .systems
            .iter()
            .find(|system| {
                system.kind == "ConveyorStation" && system.object.as_deref() == Some("object-6641")
            })
            .expect("authored conveyor");
        let source = conveyor.object.clone().expect("source");
        let target = conveyor.target_object.clone().expect("adjacent receiver");
        assert_eq!(target, "object-6248");
        let item = session.item(
            "Test tomato".to_owned(),
            ItemBody::Food {
                order: Some("Tomato_Chopped".to_owned()),
                process: None,
                work_progress_ms: 0,
            },
        );
        session.slots.insert(source.clone(), item);
        session.refresh_conveyors().expect("begin transfer");
        let (midpoint, due) = session
            .conveyor_transfers
            .iter()
            .find(|transfer| transfer.source == source)
            .map(|transfer| (transfer.midpoint_ms, transfer.due_ms))
            .expect("active transfer");
        assert_eq!(due, 1_250);

        session
            .advance_to(midpoint - 1)
            .expect("before ownership transfer");
        assert!(session.slots.contains_key(&source));
        assert!(!session.slots.contains_key(&target));

        session.advance_to(midpoint).expect("ownership transfer");
        assert!(!session.slots.contains_key(&source));
        assert!(session.slots.contains_key(&target));
        assert!(
            session
                .conveyor_transfers
                .iter()
                .any(|transfer| transfer.target == target && transfer.crossed_midpoint)
        );

        session.advance_to(due).expect("arrival");
        assert!(
            session
                .conveyor_transfers
                .iter()
                .all(|transfer| transfer.source != source)
        );
        assert!(
            session
                .conveyor_transfers
                .iter()
                .any(|transfer| transfer.source == target),
            "the next authored belt segment starts only after arrival"
        );
    }

    #[test]
    fn looping_authored_counter_motion_updates_collision_occupancy() {
        let data = data(16);
        let mut session = Session::new(
            &data,
            SessionConfig {
                time_scale: 1,
                seed: 1,
            },
        )
        .expect("session");
        session.start().expect("start");
        let object = session.object("object-2430").expect("moving counter");
        let initial_world = session.object_world(object);
        let initial_cell = nearest_cell(&session.data.layout.walkable, initial_world)
            .expect("walkable cell under moving counter");
        assert!(session.cell_blocked(initial_cell));

        session.advance_to(58_500).expect("counter bank moved");
        let moved_world = session.object_world(session.object("object-2430").unwrap());
        assert!(distance(initial_world, moved_world) > 4.0);
        assert!(!session.cell_blocked(initial_cell));
        let moved_cell = nearest_cell(&session.data.layout.walkable, moved_world)
            .expect("walkable cell at new counter position");
        assert!(session.cell_blocked(moved_cell));
    }

    #[test]
    fn standing_on_and_leaving_an_authored_pressure_plate_drives_its_door() {
        let data = data(22);
        let mut session = Session::new(
            &data,
            SessionConfig {
                time_scale: 1,
                seed: 1,
            },
        )
        .expect("session");
        session.start().expect("start");
        assert_eq!(session.motions["animator-2545"].state, 4);
        let door_panels = session
            .data
            .layout
            .objects
            .iter()
            .filter(|object| object.motion.as_deref() == Some("animator-2545"))
            .map(|object| {
                (
                    object.id.clone(),
                    session.object_world(object),
                    nearest_cell(&session.data.layout.walkable, session.object_world(object))
                        .expect("walkable cell at closed door"),
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(door_panels.len(), 4);
        let snapshot = session.snapshot();
        assert!(door_panels.iter().all(|(id, _, _)| {
            snapshot
                .map
                .objects
                .iter()
                .find(|object| object.id == *id)
                .is_some_and(|object| object.kind == "moving_barrier")
        }));
        assert!(
            door_panels
                .iter()
                .any(|(_, _, cell)| session.cell_blocked(*cell))
        );
        let zone = session
            .data
            .layout
            .systems
            .iter()
            .find(|system| system.id == "system-3176")
            .expect("authored pressure plate")
            .world;

        walk_until(&mut session, |world| distance(world, zone) < 0.7);
        assert!(session.occupied_zones.contains("system-3176"));
        assert!(matches!(
            session.motions["animator-2545"].values.get("IsOpen"),
            Some(MotionValue::Bool(true))
        ));
        assert_eq!(session.motions["animator-2545"].state, 1);

        session.advance_to(1_500).expect("door opens");
        assert_eq!(session.motions["animator-2545"].state, 3);
        assert!(door_panels.iter().any(|(id, initial, _)| {
            distance(
                *initial,
                session.object_world(session.object(id).expect("door panel")),
            ) > 0.5
        }));
        assert!(
            door_panels
                .iter()
                .any(|(_, _, cell)| !session.cell_blocked(*cell)),
            "an open authored airlock must clear its former doorway"
        );
        let direction = [
            Direction::North,
            Direction::South,
            Direction::East,
            Direction::West,
        ]
        .into_iter()
        .find(|direction| {
            session
                .movement_neighbor(session.active_chef().cell, *direction)
                .is_some_and(|cell| distance(session.cell_world(cell), zone) >= 0.7)
        })
        .expect("path off pressure plate");
        session.move_chef(direction, false).expect("leave plate");
        assert!(!session.occupied_zones.contains("system-3176"));
        assert!(matches!(
            session.motions["animator-2545"].values.get("IsOpen"),
            Some(MotionValue::Bool(false))
        ));
        assert_eq!(session.motions["animator-2545"].state, 2);
    }

    #[test]
    fn authored_hazard_activation_and_curves_schedule_meteors_and_fireballs() {
        let data = data(21);
        let mut session = Session::new(
            &data,
            SessionConfig {
                time_scale: 1,
                seed: 1,
            },
        )
        .expect("session");
        session.start().expect("start");

        assert_eq!(session.meteor_managers.len(), 1);
        let meteor_due = session.meteor_managers[0].next_spawn_ms;
        assert!((1_000..=2_000).contains(&meteor_due));
        assert_eq!(session.fireball_spawners.len(), 1);
        assert_eq!(session.fireball_spawners[0].next_spawn_ms, 667);

        session.advance_to(666).expect("before fire command");
        assert!(session.fireballs.is_empty());
        session
            .advance_to(667)
            .expect("authored command curve rises");
        assert_eq!(session.fireballs.len(), 1);
        assert_eq!(session.fireball_spawners[0].next_spawn_ms, 3_667);

        session.advance_to(meteor_due).expect("meteor warning");
        assert_eq!(session.meteors.len(), 1);
        assert_eq!(session.meteors[0].impact_ms, meteor_due + 6_000);
        assert!(
            session
                .snapshot()
                .recent_events
                .iter()
                .any(|event| event.kind == "meteor_warning")
        );
    }

    #[test]
    fn fireball_hit_drops_the_item_and_blocks_only_that_chef_until_authored_respawn() {
        let data = data(21);
        let mut session = Session::new(
            &data,
            SessionConfig {
                time_scale: 1,
                seed: 1,
            },
        )
        .expect("session");
        session.start().expect("start");
        let drop_surface = session
            .data
            .layout
            .objects
            .iter()
            .find(|object| object.has("AttachStation") && !session.slots.contains_key(&object.id))
            .expect("empty authored surface");
        let cell = nearest_cell(
            &session.data.layout.walkable,
            session.object_world(drop_surface),
        )
        .expect("walkable cell beside surface");
        session.chefs[0].cell = cell;
        session.chefs[0].held = Some(session.item(
            "Held tomato".to_owned(),
            ItemBody::Food {
                order: Some("Tomato_Chopped".to_owned()),
                process: None,
                work_progress_ms: 0,
            },
        ));
        let chef_world = session.cell_world(cell);
        session.fireballs.push(Fireball {
            id: "test-fireball".to_owned(),
            from: Vector3 {
                x: chef_world.x,
                y: chef_world.y,
                z: chef_world.z - 2.0,
            },
            to: Vector3 {
                x: chef_world.x,
                y: chef_world.y,
                z: chef_world.z + 2.0,
            },
            spawned_ms: 0,
            due_ms: 1_000,
        });
        let hit_ms = session
            .fireball_collision(&session.fireballs[0])
            .expect("projectile crosses chef")
            .0;
        session.advance_to(hit_ms).expect("fireball hits");

        assert!(session.chefs[0].held.is_none());
        assert!(
            session
                .slots
                .values()
                .any(|item| item.name == "Held tomato")
                || session
                    .loose_items
                    .values()
                    .any(|loose| loose.item.name == "Held tomato")
        );
        let respawn_due = session.chefs[0].respawn_due_ms.expect("respawning");
        let spawn = &session.data.layout.players[0];
        assert_eq!(
            respawn_due,
            hit_ms + seconds_to_ms(spawn.respawn_seconds + spawn.spawn_effect_seconds)
        );
        assert_eq!(session.score, 0);
        assert!(session.move_chef(Direction::North, false).is_err());

        session.switch().expect("control the other chef");
        let direction = [
            Direction::North,
            Direction::South,
            Direction::East,
            Direction::West,
        ]
        .into_iter()
        .find(|direction| {
            session
                .movement_neighbor(session.active_chef().cell, *direction)
                .is_some()
        })
        .expect("other chef can move");
        session
            .move_chef(direction, false)
            .expect("other chef remains controllable");
        session
            .advance_to(respawn_due)
            .expect("authored respawn delay");
        session.switch().expect("return to respawned chef");
        assert!(session.chefs[0].respawn_due_ms.is_none());
    }

    #[test]
    fn walking_off_an_authored_open_edge_destroys_the_item_and_respawns_the_chef() {
        let data = data(21);
        let mut session = Session::new(
            &data,
            SessionConfig {
                time_scale: 1,
                seed: 4,
            },
        )
        .expect("session");
        session.start().expect("start");
        let edge = session
            .data
            .layout
            .fall_edges
            .first()
            .expect("authored open edge")
            .clone();
        let destination = session
            .data
            .layout
            .walkable
            .iter()
            .position(|cell| {
                cell.grid_manager == edge.grid_manager
                    && cell.x == edge.from.x
                    && cell.y == edge.from.y
                    && cell.z == edge.from.z
            })
            .expect("edge starts on walkable ground");
        let destination_world = session.cell_world(destination);
        walk_until(&mut session, |world| {
            distance(world, destination_world) < 0.1
        });
        session.chefs[0].held = Some(session.item(
            "Doomed tomato".to_owned(),
            ItemBody::Food {
                order: Some("Tomato_Chopped".to_owned()),
                process: None,
                work_progress_ms: 0,
            },
        ));
        let direction = match (edge.dx, edge.dz) {
            (1, 0) => Direction::East,
            (-1, 0) => Direction::West,
            (0, 1) => Direction::North,
            (0, -1) => Direction::South,
            _ => panic!("cardinal fall edge"),
        };
        session
            .move_chef(direction, false)
            .expect("explicit move walks off open edge");
        assert!(session.chefs[0].respawn_due_ms.is_some());
        assert!(session.chefs[0].held.is_none());
        assert!(
            session
                .slots
                .values()
                .all(|item| item.name != "Doomed tomato")
                && session
                    .loose_items
                    .values()
                    .all(|loose| loose.item.name != "Doomed tomato")
        );
        assert_eq!(session.score, 0);
    }

    #[test]
    fn authored_extinguisher_clears_fire_after_half_a_second_of_held_input() {
        let data = data(21);
        let mut session = Session::new(
            &data,
            SessionConfig {
                time_scale: 1,
                seed: 2,
            },
        )
        .expect("session");
        session.start().expect("start");
        session.meteor_managers.clear();
        session.fireball_spawners.clear();
        let extinguisher_slot = session
            .loose_items
            .iter()
            .find_map(|(id, loose)| {
                matches!(loose.item.body, ItemBody::Extinguisher { .. }).then(|| id.clone())
            })
            .expect("authored extinguisher");
        let extinguisher_world = session.loose_items[&extinguisher_slot].world;
        walk_until(&mut session, |world| {
            distance(world, extinguisher_world) <= INTERACTION_DISTANCE
        });
        session
            .interact(&extinguisher_slot)
            .expect("take extinguisher");
        let fire_target = session
            .data
            .layout
            .objects
            .iter()
            .find(|object| object.has("Flammable"))
            .expect("flammable authored station")
            .id
            .clone();
        let fire_cell = nearest_cell(
            &session.data.layout.walkable,
            session.object_world(session.object(&fire_target).unwrap()),
        )
        .expect("cell near fire");
        session.chefs[session.active_chef].cell = fire_cell;
        session.ignite(&fire_target);
        assert!(session.interact(&fire_target).is_err());

        let due = session
            .start_work(&fire_target)
            .expect("hold extinguisher input");
        assert_eq!(due - session.elapsed_ms, 500);
        session
            .advance_to(session.elapsed_ms + 250)
            .expect("partially spray the fire");
        session.stop_work().expect("release the spray");
        let partial_strength = session.fires[&fire_target].strength;
        assert!((partial_strength - 0.5).abs() < 0.01);
        session
            .advance_to(session.elapsed_ms + 2_000)
            .expect("original suppression window");
        assert!((session.fires[&fire_target].strength - partial_strength).abs() < 0.01);
        session
            .advance_to(session.elapsed_ms + 2_500)
            .expect("original five-second fire recovery");
        assert!((session.fires[&fire_target].strength - 1.0).abs() < 0.01);

        let due = session
            .start_work(&fire_target)
            .expect("resume held extinguisher input");
        session.advance_to(due).expect("spray finishes");
        assert!(!session.fires.contains_key(&fire_target));
        assert!(
            session
                .snapshot()
                .recent_events
                .iter()
                .any(|event| event.kind == "fire_extinguished")
        );
    }

    #[test]
    fn cooking_fire_and_spread_are_independent_of_clock_step_size() {
        fn run(step_ms: u64) -> Value {
            let data = data(6);
            let mut session = Session::new(
                &data,
                SessionConfig {
                    time_scale: 5,
                    seed: 448_516,
                },
            )
            .expect("session");
            session.start().expect("start");
            let cooker = session
                .data
                .layout
                .objects
                .iter()
                .find(|object| {
                    object.feature("CookingStation").is_some()
                        && matches!(
                            session.slots.get(&object.id),
                            Some(Item {
                                body: ItemBody::Container { .. },
                                ..
                            })
                        )
                })
                .expect("authored cooker")
                .id
                .clone();
            let burn_after = match &mut session.slots.get_mut(&cooker).expect("cooking pot").body {
                ItemBody::Container {
                    contents,
                    cooking: Some(cooking),
                    ..
                } => {
                    contents.extend(["Onion".to_owned(), "Onion".to_owned(), "Onion".to_owned()]);
                    cooking.duration_ms.saturating_mul(2).saturating_add(1)
                }
                _ => panic!("authored cooking pot"),
            };
            let spread_after = session.scale(seconds_to_ms(
                session
                    .data
                    .variant
                    .config
                    .fire
                    .as_ref()
                    .expect("fire config")
                    .flammability_seconds,
            ));
            let target = burn_after + spread_after;
            while session.elapsed_ms < target {
                session
                    .advance_to((session.elapsed_ms + step_ms).min(target))
                    .expect("advance kitchen");
            }
            serde_json::to_value(session.snapshot()).expect("serialize snapshot")
        }

        assert_eq!(run(u64::MAX), run(1_000));
    }

    #[test]
    fn incomplete_recipe_does_not_cook_or_burn() {
        let data = data(2);
        let mut session = Session::new(
            &data,
            SessionConfig {
                time_scale: 1,
                seed: 1,
            },
        )
        .expect("session");
        session.start().expect("start");
        let duration = match &mut session
            .slots
            .get_mut("object-3526")
            .expect("authored pot")
            .body
        {
            ItemBody::Container {
                contents,
                cooking: Some(cooking),
                ..
            } => {
                contents.push("Onion".to_owned());
                cooking.duration_ms
            }
            _ => panic!("authored pot on cooker"),
        };
        session
            .advance_to(duration.saturating_mul(3))
            .expect("time passes with an incomplete soup");
        assert!(session.fires.is_empty());
        assert!(matches!(
            &session.slots["object-3526"].body,
            ItemBody::Container {
                cooking: Some(Cooking { progress_ms: 0, .. }),
                ..
            }
        ));
    }

    #[test]
    fn extinguished_burnt_pot_does_not_ignite_again_without_new_food() {
        let data = data(6);
        let mut session = Session::new(
            &data,
            SessionConfig {
                time_scale: 5,
                seed: 448_516,
            },
        )
        .expect("session");
        session.start().expect("start");
        let cooker = session
            .data
            .layout
            .objects
            .iter()
            .find(|object| {
                object.feature("CookingStation").is_some()
                    && matches!(
                        session.slots.get(&object.id),
                        Some(Item {
                            body: ItemBody::Container { .. },
                            ..
                        })
                    )
            })
            .expect("authored cooker")
            .id
            .clone();
        let ignition = match &mut session.slots.get_mut(&cooker).expect("cooking pot").body {
            ItemBody::Container {
                contents,
                cooking: Some(cooking),
                ..
            } => {
                contents.extend(["Onion".to_owned(), "Onion".to_owned(), "Onion".to_owned()]);
                cooking.duration_ms.saturating_mul(2).saturating_add(1)
            }
            _ => panic!("authored cooking pot"),
        };
        session.advance_to(ignition).expect("pot catches fire");
        assert!(session.fires.remove(&cooker).is_some());
        session
            .advance_to(ignition + 10_000)
            .expect("time passes after extinguishing");
        assert!(!session.fires.contains_key(&cooker));
        assert_eq!(
            session
                .events
                .iter()
                .filter(|event| event.kind == "fire_ignited" && event.message.contains(&cooker))
                .count(),
            1
        );
    }

    #[test]
    fn fire_spreads_to_an_authored_neighbor_after_the_original_flammability_time() {
        let data = data(21);
        let mut session = Session::new(
            &data,
            SessionConfig {
                time_scale: 1,
                seed: 3,
            },
        )
        .expect("session");
        session.start().expect("start");
        let pair = session
            .data
            .layout
            .objects
            .iter()
            .filter(|object| object.has("Flammable"))
            .find_map(|source| {
                session
                    .data
                    .layout
                    .objects
                    .iter()
                    .find(|target| {
                        target.has("Flammable")
                            && target.id != source.id
                            && target.grid_manager == source.grid_manager
                            && (target.grid.x - source.grid.x).abs() <= 1
                            && target.grid.y == source.grid.y
                            && (target.grid.z - source.grid.z).abs() <= 1
                    })
                    .map(|target| (source.id.clone(), target.id.clone()))
            })
            .expect("adjacent flammable stations");
        session.ignite(&pair.0);
        let spread_ms = seconds_to_ms(
            session
                .data
                .variant
                .config
                .fire
                .as_ref()
                .expect("fire config")
                .flammability_seconds,
        );
        session
            .advance_to(spread_ms - 1)
            .expect("before fire spreads");
        assert!(!session.fires.contains_key(&pair.1));
        session.advance_to(spread_ms).expect("fire spreads");
        assert!(session.fires.contains_key(&pair.1));
    }

    #[test]
    fn final_boss_waits_for_authored_platform_motion_and_five_second_intermissions() {
        let data = data(30);
        let mut session = Session::new(
            &data,
            SessionConfig {
                time_scale: 1,
                seed: 5,
            },
        )
        .expect("session");
        session.start().expect("start");
        assert!(!session.boss_ready);
        assert!(matches!(
            session.boss_transition,
            Some(BossTransition::Lowering { .. })
        ));

        while !session.boss_ready {
            session
                .advance_to(session.elapsed_ms + 250)
                .expect("initial platform lowers");
        }
        assert_eq!(session.boss_phase, 0);
        assert!(!session.orders.is_empty());

        session.orders.clear();
        session.boss_phase_index = session.data.variant.config.phases[0].len();
        session.process_due().expect("phase completed");
        let intermission_due = match session.boss_transition {
            Some(BossTransition::Intermission { due_ms }) => due_ms,
            _ => panic!("five-second intermission"),
        };
        assert_eq!(intermission_due - session.elapsed_ms, 5_000);
        session
            .advance_to(intermission_due - 1)
            .expect("during intermission");
        assert!(matches!(
            session.boss_transition,
            Some(BossTransition::Intermission { .. })
        ));
        session
            .advance_to(intermission_due)
            .expect("old platform starts rising");
        assert!(matches!(
            session.boss_transition,
            Some(BossTransition::Raising { .. })
        ));
        while session.boss_phase == 0 || !session.boss_ready {
            session
                .advance_to(session.elapsed_ms + 250)
                .expect("old platform rises and next lowers");
        }
        assert_eq!(session.boss_phase, 1);
        assert!(!session.orders.is_empty());

        session.orders.clear();
        session.boss_phase = 3;
        session.boss_phase_index = session.data.variant.config.phases[3].len();
        session.boss_ready = true;
        session.boss_transition = None;
        session.process_due().expect("last phase completed");
        assert!(session.boss_complete);
        assert_eq!(session.snapshot().shift.status, "complete");
        assert_eq!(session.snapshot().campaign.stars, 3);
        assert!(session.elapsed_ms < session.duration_ms());
    }
}
