use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

use crate::{
    Campaign, CampaignEntries, CampaignEntry, Coord, Direction, Entity, EntityType, Game3d,
    GameState, PhysicsWorld,
};

pub const CAMPAIGN_ID: &str = "stephens-sausage-roll-complete";
const MAP_VIEW_RADIUS: i32 = 14;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SessionRecord {
    pub schema: String,
    pub solved: usize,
    pub histories: Vec<Vec<Direction>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub overworld_histories: Option<Vec<Vec<Direction>>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub in_level: Option<bool>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct CampaignStatus {
    pub id: &'static str,
    pub score: usize,
    pub solved: usize,
    pub total: usize,
    pub complete: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct LevelStatus {
    pub ordinal: usize,
    pub id: String,
    pub title: String,
    pub status: String,
    pub actions: usize,
    pub tile_set: i32,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct EntityView {
    pub id: i32,
    pub kind: EntityType,
    pub pos: Coord,
    pub direction: Direction,
    pub rotation: i32,
    pub pivot: i32,
    pub cells: Vec<Coord>,
    pub attached_to: Option<i32>,
    pub cooked_faces: Option<[i32; 4]>,
    pub island: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct TileView {
    pub pos: Coord,
    pub kind: String,
    pub source_id: i32,
    pub direction: Direction,
    pub tile_number: i32,
    pub tile_set: i32,
    pub variant: i32,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ExitView {
    pub pos: Coord,
    pub direction: Direction,
    pub ready: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct BoundsView {
    pub min: Coord,
    pub max: Coord,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct EntranceView {
    pub ordinal: usize,
    pub id: String,
    pub title: String,
    pub pos: Coord,
    pub direction: Direction,
    pub island_id: i32,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct IslandPoseView {
    pub id: i32,
    pub pos: Coord,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct OverworldStatus {
    pub title: String,
    pub actions: usize,
    pub bounds: BoundsView,
    pub target: Option<EntranceView>,
    pub completed_islands: Vec<i32>,
    pub islands: Vec<IslandPoseView>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct OverworldMapView {
    pub bounds: BoundsView,
    pub entrances: Vec<EntranceView>,
    pub islands: Vec<IslandPoseView>,
    pub tiles: Vec<TileView>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct GameSnapshot {
    pub schema: &'static str,
    pub campaign: CampaignStatus,
    pub status: String,
    pub mode: &'static str,
    pub level: Option<LevelStatus>,
    pub overworld: Option<OverworldStatus>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub overworld_map: Option<OverworldMapView>,
    pub exit_ready: bool,
    pub exit: Option<ExitView>,
    pub player: Option<EntityView>,
    pub entities: Vec<EntityView>,
    pub tiles: Vec<TileView>,
    pub controls: [&'static str; 4],
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct MoveResult {
    pub requested: usize,
    pub applied: usize,
    pub accepted: Vec<bool>,
    pub entered_levels: Vec<String>,
    pub solved_levels: Vec<String>,
    pub snapshot: GameSnapshot,
}

pub struct Session<'a> {
    campaign: &'a Campaign,
    entries: &'a CampaignEntries,
    histories: Vec<Vec<Direction>>,
    overworld_histories: Option<Vec<Vec<Direction>>>,
    solved: usize,
    overworld: Option<Game3d<'a>>,
    game: Option<Game3d<'a>>,
}

impl<'a> Session<'a> {
    pub fn new(campaign: &'a Campaign, entries: &'a CampaignEntries) -> Result<Self, String> {
        entries.validate_against(campaign)?;
        let histories = vec![Vec::new(); entries.levels.len()];
        let overworld_histories = Some(vec![Vec::new(); entries.levels.len() + 1]);
        let overworld = Some(initial_overworld(campaign)?);
        Ok(Self {
            campaign,
            entries,
            histories,
            overworld_histories,
            solved: 0,
            overworld,
            game: None,
        })
    }

    pub fn restore(
        campaign: &'a Campaign,
        entries: &'a CampaignEntries,
        record: SessionRecord,
    ) -> Result<Self, String> {
        entries.validate_against(campaign)?;
        if record.histories.len() != entries.levels.len() || record.solved > entries.levels.len() {
            return Err("saved session has invalid campaign dimensions".to_owned());
        }
        match record.schema.as_str() {
            "sausage-session-v1" if record.overworld_histories.is_none() => {
                let mut session = Self {
                    campaign,
                    entries,
                    histories: record.histories,
                    overworld_histories: None,
                    solved: record.solved,
                    overworld: None,
                    game: None,
                };
                session.validate_solved_histories()?;
                if session.solved < entries.levels.len() {
                    session.game = Some(session.replay_level(session.solved)?);
                }
                Ok(session)
            }
            "sausage-session-v2" => {
                let map_histories = record
                    .overworld_histories
                    .ok_or_else(|| "v2 saved session is missing overworld histories".to_owned())?;
                if map_histories.len() != entries.levels.len() + 1 {
                    return Err("saved session has invalid overworld dimensions".to_owned());
                }
                let mut session = Self {
                    campaign,
                    entries,
                    histories: record.histories,
                    overworld_histories: Some(map_histories),
                    solved: record.solved,
                    overworld: None,
                    game: None,
                };
                session.rebuild_overworld()?;
                if record.in_level.unwrap_or(false) {
                    if session.solved == entries.levels.len() {
                        return Err("completed campaign cannot be saved inside a level".to_owned());
                    }
                    let state = prepare_level_entry(
                        campaign,
                        &entries.levels[session.solved],
                        session.overworld.as_mut().expect("restored map exists"),
                    )?;
                    session.game = Some(replay_entry_state(
                        campaign,
                        &entries.levels[session.solved],
                        &state,
                        &session.histories[session.solved],
                    )?);
                }
                Ok(session)
            }
            schema => Err(format!("unknown session schema {schema:?}")),
        }
    }

    pub fn record(&self) -> SessionRecord {
        SessionRecord {
            schema: if self.overworld_histories.is_some() {
                "sausage-session-v2"
            } else {
                "sausage-session-v1"
            }
            .to_owned(),
            solved: self.solved,
            histories: self.histories.clone(),
            overworld_histories: self.overworld_histories.clone(),
            in_level: self
                .overworld_histories
                .as_ref()
                .map(|_| self.game.is_some()),
        }
    }

    pub fn snapshot(&self) -> Result<GameSnapshot, String> {
        self.snapshot_with_map(false)
    }

    pub fn observer_snapshot(&self) -> Result<GameSnapshot, String> {
        self.snapshot_with_map(true)
    }

    fn snapshot_with_map(&self, include_map: bool) -> Result<GameSnapshot, String> {
        let total = self.entries.levels.len();
        let campaign = CampaignStatus {
            id: CAMPAIGN_ID,
            score: self.solved,
            solved: self.solved,
            total,
            complete: self.solved == total,
        };
        if let Some(game) = &self.game {
            let entry = &self.entries.levels[self.solved];
            let level = LevelStatus {
                ordinal: entry.ordinal,
                id: entry.id.clone(),
                title: level_title(self.campaign, &entry.id)?,
                status: if game.won() { "cooked" } else { "in_progress" }.to_owned(),
                actions: self.histories[self.solved].len(),
                tile_set: level_tile_set(game, &entry.id),
            };
            return snapshot_from_game(game, campaign, level);
        }
        if let Some(overworld) = &self.overworld {
            let level = self
                .entries
                .levels
                .get(self.solved)
                .map(|entry| -> Result<LevelStatus, String> {
                    Ok(LevelStatus {
                        ordinal: entry.ordinal,
                        id: entry.id.clone(),
                        title: level_title(self.campaign, &entry.id)?,
                        status: "available".to_owned(),
                        actions: self.histories[self.solved].len(),
                        tile_set: entry_tile_set(entry),
                    })
                })
                .transpose()?;
            let (player, entities, tiles) = project_overworld(&overworld.world)?;
            let bounds = world_bounds(&overworld.world)?;
            let target = self
                .entries
                .levels
                .get(self.solved)
                .map(|entry| entrance_view(self.campaign, &overworld.world, entry))
                .transpose()?;
            let completed_islands = overworld
                .world
                .entities
                .iter()
                .filter(|entity| entity.entity_type == EntityType::Island && entity.cook_data != 0)
                .map(|entity| entity.id)
                .collect();
            return Ok(GameSnapshot {
                schema: "sausage-state-v2",
                campaign,
                status: if self.solved == total {
                    "complete"
                } else {
                    "in_progress"
                }
                .to_owned(),
                mode: "overworld",
                level,
                overworld: Some(OverworldStatus {
                    title: "Land's End".to_owned(),
                    actions: self
                        .overworld_histories
                        .as_ref()
                        .map_or(0, |histories| histories[self.solved].len()),
                    bounds: bounds.clone(),
                    target,
                    completed_islands,
                    islands: island_poses(&overworld.world),
                }),
                overworld_map: include_map
                    .then(|| overworld_map(self.campaign, self.entries))
                    .transpose()?,
                exit_ready: false,
                exit: None,
                player,
                entities,
                tiles,
                controls: ["north", "south", "west", "east"],
            });
        }
        Ok(GameSnapshot {
            schema: "sausage-state-v2",
            campaign,
            status: "complete".to_owned(),
            mode: "complete",
            level: None,
            overworld: None,
            overworld_map: None,
            exit_ready: false,
            exit: None,
            player: None,
            entities: Vec::new(),
            tiles: Vec::new(),
            controls: ["north", "south", "west", "east"],
        })
    }

    pub fn levels(&self) -> Result<Vec<LevelStatus>, String> {
        self.entries
            .levels
            .iter()
            .enumerate()
            .map(|(index, entry)| {
                Ok(LevelStatus {
                    ordinal: entry.ordinal,
                    id: entry.id.clone(),
                    title: level_title(self.campaign, &entry.id)?,
                    status: if index < self.solved {
                        "solved"
                    } else if index == self.solved && self.game.is_some() {
                        "current"
                    } else if index == self.solved {
                        "available"
                    } else {
                        "locked"
                    }
                    .to_owned(),
                    actions: self.histories[index].len(),
                    tile_set: entry_tile_set(entry),
                })
            })
            .collect()
    }

    pub fn move_many(&mut self, directions: &[Direction]) -> Result<MoveResult, String> {
        if directions.is_empty() {
            return Err("move requires at least one direction".to_owned());
        }
        let requested = directions.len();
        let mut accepted = Vec::new();
        let mut entered_levels = Vec::new();
        let mut solved_levels = Vec::new();
        for direction in directions {
            if let Some(game) = self.game.as_mut() {
                let index = self.solved;
                let did_move = game.step(*direction)?;
                self.histories[index].push(*direction);
                accepted.push(did_move);
                if game.can_exit() {
                    let exit_player = game
                        .world
                        .entity(game.world.player_id())
                        .ok_or_else(|| "player disappeared at the level exit".to_owned())?
                        .clone();
                    let island_deltas = completion_island_deltas(
                        &self.entries.levels[index].state,
                        &game.world,
                        &self.entries.levels[index].id,
                    )?;
                    solved_levels.push(self.entries.levels[index].id.clone());
                    self.solved += 1;
                    if let Some(overworld) = self.overworld.as_mut() {
                        apply_completion(
                            self.campaign,
                            self.entries,
                            overworld,
                            index,
                            &exit_player,
                            &island_deltas,
                        )?;
                        self.game = None;
                    } else {
                        self.game = self
                            .entries
                            .levels
                            .get(self.solved)
                            .map(|entry| Game3d::from_state(self.campaign, &entry.state))
                            .transpose()?;
                    }
                    break;
                }
                continue;
            }
            let overworld = self
                .overworld
                .as_mut()
                .ok_or_else(|| "campaign is already complete".to_owned())?;
            let did_move = overworld.step(*direction)?;
            self.overworld_histories
                .as_mut()
                .expect("overworld session has map histories")[self.solved]
                .push(*direction);
            accepted.push(did_move);
            if let Some(entry) = self.entries.levels.get(self.solved)
                && at_entrance(self.campaign, overworld, entry)?
            {
                entered_levels.push(entry.id.clone());
                let state = prepare_level_entry(self.campaign, entry, overworld)?;
                self.game = Some(Game3d::from_state(self.campaign, &state)?);
                break;
            }
        }
        Ok(MoveResult {
            requested,
            applied: accepted.len(),
            accepted,
            entered_levels,
            solved_levels,
            snapshot: self.snapshot()?,
        })
    }

    pub fn undo(&mut self, count: usize) -> Result<usize, String> {
        if count == 0 {
            return Err("undo count must be positive".to_owned());
        }
        if self.overworld_histories.is_none() {
            return self.undo_legacy(count);
        }
        let mut undone = 0;
        for _ in 0..count {
            if self.game.is_some() && self.histories[self.solved].pop().is_some() {
                self.rebuild_overworld()?;
                let state = prepare_level_entry(
                    self.campaign,
                    &self.entries.levels[self.solved],
                    self.overworld.as_mut().expect("rebuilt map exists"),
                )?;
                self.game = Some(replay_entry_state(
                    self.campaign,
                    &self.entries.levels[self.solved],
                    &state,
                    &self.histories[self.solved],
                )?);
                undone += 1;
                continue;
            }
            if self.game.is_some() {
                self.game = None;
            }
            if self.overworld_histories.as_mut().expect("map histories")[self.solved]
                .pop()
                .is_some()
            {
                self.rebuild_overworld()?;
                undone += 1;
                continue;
            }
            if self.solved == 0 {
                break;
            }
            self.solved -= 1;
            if self.histories[self.solved].pop().is_none() {
                return Err(format!(
                    "solved level {} has no action to undo",
                    self.solved + 1
                ));
            }
            self.rebuild_overworld()?;
            let state = prepare_level_entry(
                self.campaign,
                &self.entries.levels[self.solved],
                self.overworld.as_mut().expect("rebuilt map exists"),
            )?;
            self.game = Some(replay_entry_state(
                self.campaign,
                &self.entries.levels[self.solved],
                &state,
                &self.histories[self.solved],
            )?);
            undone += 1;
        }
        Ok(undone)
    }

    pub fn restart(&mut self) -> Result<(), String> {
        if self.overworld_histories.is_none() {
            if self.solved == self.entries.levels.len() {
                return Err("campaign is already complete".to_owned());
            }
            self.histories[self.solved].clear();
            self.game = Some(Game3d::from_state(
                self.campaign,
                &self.entries.levels[self.solved].state,
            )?);
            return Ok(());
        }
        if self.game.is_some() {
            self.histories[self.solved].clear();
            self.rebuild_overworld()?;
            let state = prepare_level_entry(
                self.campaign,
                &self.entries.levels[self.solved],
                self.overworld.as_mut().expect("rebuilt map exists"),
            )?;
            self.game = Some(Game3d::from_state(self.campaign, &state)?);
        } else {
            self.overworld_histories.as_mut().expect("map histories")[self.solved].clear();
            self.rebuild_overworld()?;
        }
        Ok(())
    }

    fn validate_solved_histories(&self) -> Result<(), String> {
        for index in 0..self.solved {
            let game = self.replay_level(index)?;
            if !game.can_exit() {
                return Err(format!(
                    "saved history for level {} is not solved at its exit",
                    index + 1
                ));
            }
        }
        Ok(())
    }

    fn rebuild_overworld(&mut self) -> Result<(), String> {
        let histories = self
            .overworld_histories
            .as_ref()
            .ok_or_else(|| "legacy session has no overworld".to_owned())?;
        let mut overworld = initial_overworld(self.campaign)?;
        for (index, map_history) in histories.iter().enumerate().take(self.solved + 1) {
            for (action, direction) in map_history.iter().copied().enumerate() {
                overworld.step(direction).map_err(|error| {
                    format!(
                        "overworld segment {} action {}: {error}",
                        index + 1,
                        action + 1
                    )
                })?;
            }
            if index < self.solved {
                let state = prepare_level_entry(
                    self.campaign,
                    &self.entries.levels[index],
                    &mut overworld,
                )?;
                let completed = replay_entry_state(
                    self.campaign,
                    &self.entries.levels[index],
                    &state,
                    &self.histories[index],
                )?;
                let exit_player = completed
                    .world
                    .entity(completed.world.player_id())
                    .ok_or_else(|| format!("level {} has no player at its exit", index + 1))?;
                let island_deltas = completion_island_deltas(
                    &state,
                    &completed.world,
                    &self.entries.levels[index].id,
                )?;
                apply_completion(
                    self.campaign,
                    self.entries,
                    &mut overworld,
                    index,
                    exit_player,
                    &island_deltas,
                )?;
            }
        }
        self.overworld = Some(overworld);
        Ok(())
    }

    fn undo_legacy(&mut self, count: usize) -> Result<usize, String> {
        let mut undone = 0;
        for _ in 0..count {
            if self.solved == self.entries.levels.len() || self.histories[self.solved].is_empty() {
                if self.solved == 0 {
                    break;
                }
                self.solved -= 1;
            }
            if self.histories[self.solved].pop().is_none() {
                break;
            }
            undone += 1;
        }
        self.game = if self.solved < self.entries.levels.len() {
            Some(self.replay_level(self.solved)?)
        } else {
            None
        };
        Ok(undone)
    }

    fn replay_level(&self, index: usize) -> Result<Game3d<'a>, String> {
        let entry = self
            .entries
            .levels
            .get(index)
            .ok_or_else(|| format!("unknown level index {index}"))?;
        let mut game = Game3d::from_state(self.campaign, &entry.state)?;
        for (action, direction) in self.histories[index].iter().copied().enumerate() {
            game.step(direction)
                .map_err(|error| format!("{} action {}: {error}", entry.id, action + 1))?;
        }
        Ok(game)
    }
}

type WorldProjection = (Option<EntityView>, Vec<EntityView>, Vec<TileView>);

pub fn replay_snapshot(
    campaign: &Campaign,
    entry: &CampaignEntry,
    total: usize,
    directions: &[Direction],
) -> Result<GameSnapshot, String> {
    let mut game = Game3d::from_state(campaign, &entry.state)?;
    for direction in directions {
        game.step(*direction)?;
    }
    let title = level_title(campaign, &entry.id)?;
    let campaign_status = CampaignStatus {
        id: CAMPAIGN_ID,
        score: entry.ordinal.saturating_sub(1),
        solved: entry.ordinal.saturating_sub(1),
        total,
        complete: false,
    };
    let level = LevelStatus {
        ordinal: entry.ordinal,
        id: entry.id.clone(),
        title,
        status: if game.won() { "cooked" } else { "in_progress" }.to_owned(),
        actions: directions.len(),
        tile_set: level_tile_set(&game, &entry.id),
    };
    snapshot_from_game(&game, campaign_status, level)
}

fn snapshot_from_game(
    game: &Game3d<'_>,
    campaign: CampaignStatus,
    level: LevelStatus,
) -> Result<GameSnapshot, String> {
    let (player, entities, tiles) = project_puzzle(&game.world, &level.id)?;
    let exit_ready = game.can_exit();
    let (exit_pos, exit_direction) = game.exit();
    Ok(GameSnapshot {
        schema: "sausage-state-v2",
        campaign,
        status: "in_progress".to_owned(),
        mode: "puzzle",
        level: Some(level),
        overworld: None,
        overworld_map: None,
        exit_ready,
        exit: Some(ExitView {
            pos: exit_pos,
            direction: exit_direction,
            ready: exit_ready,
        }),
        player,
        entities,
        tiles,
        controls: ["north", "south", "west", "east"],
    })
}

fn at_entrance(
    campaign: &Campaign,
    game: &Game3d<'_>,
    entry: &CampaignEntry,
) -> Result<bool, String> {
    let positioned = campaign
        .player_positions
        .get(&entry.id)
        .ok_or_else(|| format!("puzzle {:?} has no entry position", entry.id))?;
    let player = game
        .world
        .entity(game.world.player_id())
        .ok_or_else(|| "player disappeared".to_owned())?;
    let island = game
        .world
        .entities
        .iter()
        .find(|entity| entity.entity_type == EntityType::Island && entity.data == entry.id)
        .ok_or_else(|| format!("overworld has no island for {:?}", entry.id))?;
    if player.pos != positioned.pos + island.pos
        || player.direction != positioned.direction
        || !game.world.player_has_fork()
        || game
            .world
            .entities
            .iter()
            .any(|entity| entity.entity_type == EntityType::Sausage && entity.cook_data != 0)
    {
        return Ok(false);
    }
    Ok(island.cook_data == 0)
}

fn prepare_level_entry(
    _campaign: &Campaign,
    entry: &CampaignEntry,
    overworld: &mut Game3d<'_>,
) -> Result<GameState, String> {
    let target_id = overworld
        .world
        .entities
        .iter()
        .find(|entity| entity.entity_type == EntityType::Island && entity.data == entry.id)
        .map(|entity| entity.id)
        .ok_or_else(|| format!("overworld has no island for {:?}", entry.id))?;
    overworld.enter_overworld_level(&entry.id)?;
    let supported = target_support_chain(&overworld.world, target_id)?;
    let map_player_id = overworld.world.player_id();
    let mut state = entry.state.clone();
    state
        .entities
        .retain(|entity| entity.entity_type != EntityType::Sausage || entity.tile_set != 1);
    let next_id = state
        .entities
        .iter()
        .map(|entity| entity.id)
        .max()
        .unwrap_or(0)
        + 1;
    let sausages = overworld
        .world
        .entities
        .iter()
        .filter(|entity| entity.entity_type == EntityType::Sausage && entity.tile_set == 1)
        .cloned()
        .collect::<Vec<_>>();
    let mut consumed = Vec::new();
    for (next_id, sausage) in (next_id..).zip(sausages) {
        let enters_target = supported.contains(&sausage.id);
        let mut transferred = sausage.clone();
        transferred.id = next_id;
        transferred.data = if enters_target { "M;;" } else { "S;;" }.to_owned();
        if enters_target {
            consumed.push(sausage.id);
            if sausage.stuck_to == map_player_id {
                transferred.stuck_to = state.player().map_or(-1, |player| player.id);
                if let Some(player) = state
                    .entities
                    .iter_mut()
                    .find(|entity| entity.entity_type == EntityType::Player)
                {
                    player.stuck_to = transferred.id;
                }
            } else {
                transferred.stuck_to = -1;
            }
        } else {
            transferred.pos.z -= 10;
            transferred.stuck_to = -1;
            if let Some(persistent) = overworld.world.entity_mut(sausage.id) {
                persistent.data = "M;;".to_owned();
                persistent.stuck_to = -1;
            }
        }
        state.entities.push(transferred);
    }
    for id in consumed {
        if overworld
            .world
            .entity(map_player_id)
            .is_some_and(|player| player.stuck_to == id)
        {
            overworld
                .world
                .entity_mut(map_player_id)
                .expect("map player exists")
                .stuck_to = -1;
        }
        overworld.world.remove_entity(id)?;
    }
    Ok(state)
}

fn target_support_chain(world: &PhysicsWorld<'_>, target_id: i32) -> Result<HashSet<i32>, String> {
    let mut supported = HashSet::from([target_id]);
    loop {
        let mut newly_supported = Vec::new();
        for entity in world.entities.iter().filter(|entity| {
            matches!(entity.entity_type, EntityType::Player | EntityType::Sausage)
                && !supported.contains(&entity.id)
        }) {
            let mut rests_on_supported = false;
            for pos in entity.lower_footprint(world.player_has_fork()) {
                for support in &supported {
                    if world.at(*support, pos, false)? {
                        rests_on_supported = true;
                        break;
                    }
                }
                if rests_on_supported {
                    break;
                }
            }
            if rests_on_supported {
                newly_supported.push(entity.id);
            }
        }
        if newly_supported.is_empty() {
            return Ok(supported);
        }
        supported.extend(newly_supported);
    }
}

fn apply_completion(
    campaign: &Campaign,
    entries: &CampaignEntries,
    overworld: &mut Game3d<'_>,
    index: usize,
    exit_player: &Entity,
    island_deltas: &[(String, Coord)],
) -> Result<(), String> {
    let player_id = overworld.world.player_id();
    let map_player = overworld
        .world
        .entity_mut(player_id)
        .ok_or_else(|| "overworld player disappeared".to_owned())?;
    map_player.pos = exit_player.pos;
    map_player.direction = exit_player.direction;
    map_player.data.clear();
    map_player.stuck_to = -1;
    map_player.rotation = 0;
    map_player.turn_direction = Direction::None;
    map_player.pivot = 0;
    for (name, delta) in island_deltas {
        let island = overworld
            .world
            .entities
            .iter_mut()
            .find(|entity| entity.entity_type == EntityType::Island && entity.data == *name)
            .ok_or_else(|| format!("overworld has no island {name:?}"))?;
        island.pos = island.pos + *delta;
    }
    for island in overworld
        .world
        .entities
        .iter_mut()
        .filter(|entity| entity.entity_type == EntityType::Island)
    {
        let checkpoint = entries.levels[index]
            .overworld
            .entities
            .iter()
            .find(|entity| entity.entity_type == EntityType::Island && entity.data == island.data)
            .ok_or_else(|| format!("overworld checkpoint lost island {:?}", island.data))?;
        island.pos.z = checkpoint.pos.z;
    }
    let checkpoint_player = entries.levels[index]
        .overworld
        .player()
        .ok_or_else(|| format!("overworld checkpoint {} has no player", index + 1))?;
    overworld
        .world
        .entity_mut(player_id)
        .expect("overworld player still exists")
        .pos
        .z = checkpoint_player.pos.z;
    let completed = entries.levels[..=index]
        .iter()
        .map(|entry| puzzle_root(&entry.id))
        .collect::<HashSet<_>>();
    for temple in campaign.temples.iter().filter(|temple| {
        temple
            .levels
            .iter()
            .all(|level| completed.contains(level.as_str()))
    }) {
        let Some(temple_island) = overworld
            .world
            .entities
            .iter()
            .find(|entity| entity.entity_type == EntityType::Island && entity.data == temple.name)
            .cloned()
        else {
            continue;
        };
        if temple_island.cook_data != 0 {
            continue;
        }
        overworld
            .world
            .entity_mut(temple_island.id)
            .expect("temple island still exists")
            .cook_data = 1;
        for reward in campaign
            .sausage_positions
            .get(&temple.name)
            .into_iter()
            .flatten()
        {
            overworld.world.insert_entity(Entity {
                pos: temple_island.pos + reward.pos,
                entity_type: EntityType::Sausage,
                id: -1,
                direction: reward.direction,
                data: String::new(),
                stuck_to: -1,
                rotation: 0,
                cook_data: 0,
                turn_direction: Direction::None,
                tile_number: 0,
                tile_set: 1,
                pivot: 0,
            })?;
        }
    }
    Ok(())
}

fn completion_island_deltas(
    entry: &GameState,
    completed: &PhysicsWorld<'_>,
    level_id: &str,
) -> Result<Vec<(String, Coord)>, String> {
    let root = puzzle_root(level_id);
    entry
        .entities
        .iter()
        .filter(|entity| {
            entity.entity_type == EntityType::Island
                && (entity.data == root || entity.data.starts_with(&format!("{root}__")))
        })
        .map(|before| {
            let after = completed
                .entities
                .iter()
                .find(|entity| {
                    entity.entity_type == EntityType::Island && entity.data == before.data
                })
                .ok_or_else(|| format!("completed level lost island {:?}", before.data))?;
            Ok((before.data.clone(), after.pos - before.pos))
        })
        .collect()
}

fn puzzle_root(id: &str) -> &str {
    id.split_once("__").map_or(id, |(root, _)| root)
}

fn replay_entry_state<'a>(
    campaign: &'a Campaign,
    entry: &CampaignEntry,
    state: &GameState,
    directions: &[Direction],
) -> Result<Game3d<'a>, String> {
    let mut game = Game3d::from_state(campaign, state)?;
    for (action, direction) in directions.iter().copied().enumerate() {
        game.step(direction)
            .map_err(|error| format!("{} action {}: {error}", entry.id, action + 1))?;
    }
    Ok(game)
}

fn project_puzzle(world: &PhysicsWorld<'_>, level_id: &str) -> Result<WorldProjection, String> {
    world
        .entity(world.player_id())
        .ok_or_else(|| "player disappeared".to_owned())?;
    let active_island = world
        .entities
        .iter()
        .find(|entity| entity.entity_type == EntityType::Island && entity.data == level_id)
        .ok_or_else(|| format!("puzzle {level_id:?} has no active island"))?;
    let family_prefix = level_id
        .split_once("__")
        .map(|(family, _)| format!("{family}__"));
    let relevant_islands = world
        .entities
        .iter()
        .filter(|entity| {
            entity.entity_type == EntityType::Island
                && (entity.id == active_island.id
                    || family_prefix
                        .as_ref()
                        .is_some_and(|prefix| entity.data.starts_with(prefix)))
        })
        .collect::<Vec<_>>();
    let relevant_tiles = relevant_islands
        .iter()
        .map(|island| {
            Ok((
                *island,
                world
                    .source_footprint(island.id)?
                    .into_iter()
                    .filter(|pos| pos.z > -10)
                    .collect::<Vec<_>>(),
            ))
        })
        .collect::<Result<Vec<_>, String>>()?;
    let mut visible_positions = relevant_tiles
        .iter()
        .flat_map(|(_, positions)| positions.iter().copied());
    let first_visible = visible_positions
        .next()
        .ok_or_else(|| format!("puzzle {level_id:?} has no visible terrain"))?;
    let mut focus_min = first_visible;
    let mut focus_max = first_visible;
    for pos in visible_positions {
        focus_min = min_coord(focus_min, pos);
        focus_max = max_coord(focus_max, pos);
    }
    let visible = |pos: Coord| {
        pos.x >= focus_min.x - 2
            && pos.x <= focus_max.x + 2
            && pos.y >= focus_min.y - 2
            && pos.y <= focus_max.y + 2
            && pos.z >= focus_min.z - 3
            && pos.z <= focus_max.z + 6
    };
    project(world, relevant_tiles, visible)
}

fn project_overworld(world: &PhysicsWorld<'_>) -> Result<WorldProjection, String> {
    let player = world
        .entity(world.player_id())
        .ok_or_else(|| "player disappeared".to_owned())?;
    let center = player.pos;
    let visible = |pos: Coord| {
        (pos.x - center.x).abs() <= MAP_VIEW_RADIUS
            && (pos.y - center.y).abs() <= MAP_VIEW_RADIUS
            && (pos.z - center.z).abs() <= 12
    };
    let islands = world
        .entities
        .iter()
        .filter(|entity| entity.entity_type == EntityType::Island)
        .map(|island| {
            Ok((
                island,
                world
                    .source_footprint(island.id)?
                    .into_iter()
                    .filter(|pos| pos.z > -10 && visible(*pos))
                    .collect(),
            ))
        })
        .collect::<Result<Vec<_>, String>>()?;
    project(world, islands, visible)
}

fn project(
    world: &PhysicsWorld<'_>,
    relevant_tiles: Vec<(&Entity, Vec<Coord>)>,
    visible: impl Fn(Coord) -> bool,
) -> Result<WorldProjection, String> {
    let mut entities = world
        .entities
        .iter()
        .filter(|entity| {
            (!entity.entity_type.is_static() || entity.entity_type == EntityType::SpectralSausage)
                && entity.entity_type != EntityType::Island
                && (visible(entity.pos) || entity.entity_type == EntityType::Sausage)
        })
        .map(|entity| entity_view(world, entity))
        .collect::<Result<Vec<_>, _>>()?;
    entities.sort_by_key(|entity| entity.id);
    let player_view = entities
        .iter()
        .find(|entity| entity.id == world.player_id())
        .cloned();

    let mut tiles = HashMap::<Coord, TileView>::new();
    for (island, positions) in relevant_tiles {
        for pos in positions {
            tiles.insert(pos, island_tile(world, island, pos)?);
        }
    }
    for entity in world.entities.iter().filter(|entity| {
        entity.entity_type.is_static()
            && entity.entity_type != EntityType::SpectralSausage
            && visible(entity.pos)
    }) {
        tiles.insert(entity.pos, static_tile(entity));
    }
    let mut tiles = tiles.into_values().collect::<Vec<_>>();
    tiles.sort_by_key(|tile| (tile.pos.z, tile.pos.y, tile.pos.x, tile.source_id));
    Ok((player_view, entities, tiles))
}

fn overworld_map(
    campaign: &Campaign,
    entries: &CampaignEntries,
) -> Result<OverworldMapView, String> {
    let game = initial_overworld(campaign)?;
    let world = &game.world;
    let mut tiles = Vec::new();
    for island in world
        .entities
        .iter()
        .filter(|entity| entity.entity_type == EntityType::Island)
    {
        for pos in world
            .source_footprint(island.id)?
            .into_iter()
            .filter(|pos| pos.z > -10)
        {
            tiles.push(island_tile(world, island, pos)?);
        }
    }
    tiles.sort_by_key(|tile| (tile.pos.z, tile.pos.y, tile.pos.x, tile.source_id));
    let entrances = entries
        .levels
        .iter()
        .map(|entry| entrance_view(campaign, world, entry))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(OverworldMapView {
        bounds: bounds_from_tiles(&tiles)?,
        entrances,
        islands: island_poses(world),
        tiles,
    })
}

fn island_poses(world: &PhysicsWorld<'_>) -> Vec<IslandPoseView> {
    let mut islands = world
        .entities
        .iter()
        .filter(|entity| entity.entity_type == EntityType::Island)
        .map(|entity| IslandPoseView {
            id: entity.id,
            pos: entity.pos,
        })
        .collect::<Vec<_>>();
    islands.sort_by_key(|island| island.id);
    islands
}

fn initial_overworld(campaign: &Campaign) -> Result<Game3d<'_>, String> {
    Game3d::from_state(campaign, &campaign.initial_overworld_state()?)
}

fn entrance_view(
    campaign: &Campaign,
    world: &PhysicsWorld<'_>,
    entry: &CampaignEntry,
) -> Result<EntranceView, String> {
    let island = world
        .entities
        .iter()
        .find(|entity| entity.entity_type == EntityType::Island && entity.data == entry.id)
        .ok_or_else(|| format!("overworld has no island for {:?}", entry.id))?;
    let positioned = campaign
        .player_positions
        .get(&entry.id)
        .ok_or_else(|| format!("puzzle {:?} has no entry position", entry.id))?;
    Ok(EntranceView {
        ordinal: entry.ordinal,
        id: entry.id.clone(),
        title: level_title(campaign, &entry.id)?,
        pos: positioned.pos + island.pos,
        direction: positioned.direction,
        island_id: island.id,
    })
}

fn world_bounds(world: &PhysicsWorld<'_>) -> Result<BoundsView, String> {
    let mut positions = Vec::new();
    for island in world
        .entities
        .iter()
        .filter(|entity| entity.entity_type == EntityType::Island)
    {
        positions.extend(
            world
                .source_footprint(island.id)?
                .into_iter()
                .filter(|pos| pos.z > -10),
        );
    }
    bounds_from_positions(positions.into_iter())
}

fn bounds_from_tiles(tiles: &[TileView]) -> Result<BoundsView, String> {
    bounds_from_positions(tiles.iter().map(|tile| tile.pos))
}

fn bounds_from_positions(mut positions: impl Iterator<Item = Coord>) -> Result<BoundsView, String> {
    let first = positions
        .next()
        .ok_or_else(|| "overworld has no visible terrain".to_owned())?;
    let (mut min, mut max) = (first, first);
    for pos in positions {
        min = min_coord(min, pos);
        max = max_coord(max, pos);
    }
    Ok(BoundsView { min, max })
}

fn min_coord(left: Coord, right: Coord) -> Coord {
    Coord::new(
        left.x.min(right.x),
        left.y.min(right.y),
        left.z.min(right.z),
    )
}

fn max_coord(left: Coord, right: Coord) -> Coord {
    Coord::new(
        left.x.max(right.x),
        left.y.max(right.y),
        left.z.max(right.z),
    )
}

fn island_tile(world: &PhysicsWorld<'_>, island: &Entity, pos: Coord) -> Result<TileView, String> {
    let (variant, _) = world.island_mask_value(island.id, pos)?;
    let (kind, direction) = match variant {
        2 => ("grill", Direction::East),
        20 => ("grill", Direction::North),
        3..=6 => ("ladder", Direction::from_i32(variant - 3)?),
        _ => ("land", island.direction),
    };
    Ok(TileView {
        pos,
        kind: kind.to_owned(),
        source_id: island.id,
        direction,
        tile_number: island.tile_number,
        tile_set: island.tile_set,
        variant,
    })
}

fn static_tile(entity: &Entity) -> TileView {
    let kind = match entity.entity_type {
        EntityType::Bbq => "grill",
        EntityType::Ladder => "ladder",
        EntityType::Ground => "ground",
        EntityType::SpectralSausage => "spectral_sausage",
        _ => "static",
    };
    TileView {
        pos: entity.pos,
        kind: kind.to_owned(),
        source_id: entity.id,
        direction: entity.direction,
        tile_number: entity.tile_number,
        tile_set: entity.tile_set,
        variant: 0,
    }
}

fn level_title(campaign: &Campaign, level_id: &str) -> Result<String, String> {
    campaign
        .island_state(level_id)
        .map(|state| state.display_name)
}

fn entity_view(world: &PhysicsWorld<'_>, entity: &Entity) -> Result<EntityView, String> {
    let cooked_faces = (entity.entity_type == EntityType::Sausage).then_some([
        entity.cook_data % 4,
        entity.cook_data / 4 % 4,
        entity.cook_data / 16 % 4,
        entity.cook_data / 64 % 4,
    ]);
    Ok(EntityView {
        id: entity.id,
        kind: entity.entity_type,
        pos: entity.pos,
        direction: entity.direction,
        rotation: entity.rotation,
        pivot: entity.pivot,
        cells: world.source_footprint(entity.id)?,
        attached_to: (entity.stuck_to >= 0).then_some(entity.stuck_to),
        cooked_faces,
        island: (entity.entity_type == EntityType::Island).then(|| entity.data.clone()),
    })
}

fn level_tile_set(game: &Game3d<'_>, level_id: &str) -> i32 {
    game.world
        .entities
        .iter()
        .find(|entity| entity.entity_type == EntityType::Island && entity.data == level_id)
        .map(|entity| entity.tile_set)
        .expect("loaded game should retain its active island")
}

fn entry_tile_set(entry: &CampaignEntry) -> i32 {
    entry
        .state
        .entities
        .iter()
        .find(|entity| entity.entity_type == EntityType::Island && entity.data == entry.id)
        .map(|entity| entity.tile_set)
        .expect("validated entry should contain its active island")
}

#[cfg(test)]
mod tests {
    use crate::{OracleCampaign, Replay};

    use super::*;

    fn inputs() -> (Campaign, CampaignEntries, OracleCampaign) {
        let root = crate::data_root();
        (
            Campaign::load_gzip(root.join("campaign").join("merged_binary.gz")).expect("campaign"),
            CampaignEntries::load(root.join("campaign").join("entries.tar.gz")).expect("entries"),
            OracleCampaign::load(root.join("oracle").join("segments.tar.gz")).expect("oracle"),
        )
    }

    #[test]
    fn new_session_starts_on_the_full_overworld() {
        let (campaign, entries, _) = inputs();
        let session = Session::new(&campaign, &entries).expect("session");
        let snapshot = session.snapshot().expect("snapshot");
        let map = session.observer_snapshot().expect("observer snapshot");

        assert_eq!(snapshot.mode, "overworld");
        assert_eq!(snapshot.level.expect("target").title, "Lachrymose Head");
        assert!(snapshot.tiles.len() < 2_000);
        let map = map.overworld_map.expect("map");
        assert_eq!(map.entrances.len(), 86);
        assert_eq!(map.islands.len(), 205);
        assert_eq!(map.tiles.len(), 16_261);
    }

    #[test]
    fn original_map_route_enters_and_solves_the_first_level() {
        let (campaign, entries, oracle) = inputs();
        let replay = Replay::load(crate::data_root().join("oracle/all.dem")).expect("full replay");
        let mut session = Session::new(&campaign, &entries).expect("session");
        let entered = session
            .move_many(&replay.directions[..15])
            .expect("map route");
        assert_eq!(
            entered.entered_levels,
            ["level47"],
            "player={:?}",
            session
                .overworld
                .as_ref()
                .and_then(|game| game.world.entity(game.world.player_id()))
        );
        assert_eq!(entered.snapshot.mode, "puzzle");

        let solved = session
            .move_many(&oracle.segments[0].replay.directions)
            .expect("walkthrough");
        assert_eq!(solved.solved_levels, ["level47"]);
        assert_eq!(solved.snapshot.mode, "overworld");
        assert_eq!(session.record().solved, 1);
    }

    #[test]
    fn shared_map_transforms_reconstruct_a_completed_overworld() {
        let (campaign, entries, oracle) = inputs();
        let replay = Replay::load(crate::data_root().join("oracle/all.dem")).expect("full replay");
        let mut session = Session::new(&campaign, &entries).expect("session");
        let shared = session
            .observer_snapshot()
            .expect("observer snapshot")
            .overworld_map
            .expect("shared map");
        session.move_many(&replay.directions[..15]).expect("enter");
        session
            .move_many(&oracle.segments[0].replay.directions)
            .expect("solve");
        let current = session.snapshot().expect("current map");
        let status = current.overworld.expect("overworld status");
        let initial_poses = shared
            .islands
            .iter()
            .map(|island| (island.id, island.pos))
            .collect::<HashMap<_, _>>();
        let current_poses = status
            .islands
            .iter()
            .map(|island| (island.id, island.pos))
            .collect::<HashMap<_, _>>();
        let completed = status.completed_islands.into_iter().collect::<HashSet<_>>();
        let reconstructed = shared
            .tiles
            .iter()
            .filter(|tile| !(completed.contains(&tile.source_id) && tile.variant == -1))
            .map(|tile| {
                let delta = current_poses[&tile.source_id] - initial_poses[&tile.source_id];
                (tile.source_id, tile.pos + delta, tile.variant)
            })
            .collect::<HashSet<_>>();
        let world = &session.overworld.as_ref().expect("map").world;
        let mut actual = HashSet::new();
        for island in world
            .entities
            .iter()
            .filter(|entity| entity.entity_type == EntityType::Island)
        {
            for pos in world
                .source_footprint(island.id)
                .expect("island footprint")
                .into_iter()
                .filter(|pos| pos.z > -10)
            {
                actual.insert((
                    island.id,
                    pos,
                    island_tile(world, island, pos).expect("tile").variant,
                ));
            }
        }
        assert_eq!(reconstructed, actual);
    }

    #[test]
    fn complete_original_replay_traverses_the_overworld_and_all_levels() {
        let (campaign, entries, oracle) = inputs();
        let replay = Replay::load(crate::data_root().join("oracle/all.dem")).expect("full replay");
        let mut session = Session::new(&campaign, &entries).expect("session");
        let mut cursor = 0;
        let mut map_actions = 0;

        for (index, segment) in oracle.segments.iter().enumerate() {
            let start = (cursor..=replay.directions.len() - segment.replay.directions.len())
                .find(|start| {
                    replay.directions[*start..*start + segment.replay.directions.len()]
                        == segment.replay.directions
                })
                .unwrap_or_else(|| panic!("could not align {}", segment.id));
            let route = &replay.directions[cursor..start];
            map_actions += route.len();
            let entered = session
                .move_many(route)
                .unwrap_or_else(|error| panic!("map route to {}: {error}", segment.id));
            assert_eq!(
                entered.entered_levels,
                [segment.id.as_str()],
                "ordinal={} player={:?} target={:?} sausages={:?}",
                segment.ordinal,
                session.overworld.as_ref().and_then(|game| game
                    .world
                    .entity(game.world.player_id())
                    .map(|player| (player.pos, player.direction))),
                session.overworld.as_ref().and_then(|game| entrance_view(
                    &campaign,
                    &game.world,
                    &entries.levels[session.solved]
                )
                .ok()),
                session.overworld.as_ref().map(|game| game
                    .world
                    .entities
                    .iter()
                    .filter(|entity| entity.entity_type == EntityType::Sausage)
                    .map(|entity| (entity.pos, entity.direction, entity.cook_data))
                    .collect::<Vec<_>>())
            );
            let solved = session
                .move_many(&segment.replay.directions)
                .unwrap_or_else(|error| panic!("puzzle {}: {error}", segment.id));
            assert_eq!(solved.solved_levels, [segment.id.as_str()]);
            let original_sausages = entries.levels[index]
                .overworld
                .entities
                .iter()
                .filter(|entity| entity.entity_type == EntityType::Sausage)
                .map(|entity| (entity.pos, entity.direction, entity.cook_data))
                .collect::<HashSet<_>>();
            let actual_sausages = session
                .overworld
                .as_ref()
                .expect("map")
                .world
                .entities
                .iter()
                .filter(|entity| entity.entity_type == EntityType::Sausage)
                .map(|entity| (entity.pos, entity.direction, entity.cook_data))
                .collect::<HashSet<_>>();
            assert_eq!(
                actual_sausages, original_sausages,
                "world sausages diverged after ordinal {} {}",
                segment.ordinal, segment.id
            );
            cursor = start + segment.replay.directions.len();
        }
        let tail = &replay.directions[cursor..];
        map_actions += tail.len();
        session.move_many(tail).expect("final overworld route");

        assert_eq!(map_actions, 4_592);
        assert_eq!(session.record().solved, entries.levels.len());
        assert_eq!(
            session.snapshot().expect("complete state").mode,
            "overworld"
        );
    }

    #[test]
    fn undo_crosses_map_and_level_boundaries() {
        let (campaign, entries, oracle) = inputs();
        let replay = Replay::load(crate::data_root().join("oracle/all.dem")).expect("full replay");
        let mut session = Session::new(&campaign, &entries).expect("session");
        session.move_many(&replay.directions[..15]).expect("enter");
        assert_eq!(session.undo(1).expect("undo entrance"), 1);
        assert_eq!(session.snapshot().expect("map").mode, "overworld");
        session.move_many(&[Direction::North]).expect("re-enter");
        session
            .move_many(&oracle.segments[0].replay.directions)
            .expect("solve");
        assert_eq!(session.undo(1).expect("undo solution"), 1);
        assert_eq!(session.snapshot().expect("puzzle").mode, "puzzle");
        assert_eq!(session.record().solved, 0);
    }

    #[test]
    fn multi_island_snapshot_keeps_the_whole_visible_puzzle_family() {
        let (campaign, entries, oracle) = inputs();
        let index = entries
            .levels
            .iter()
            .position(|entry| entry.id == "islandshape43s__island1")
            .expect("Wobblecliff entry");
        let snapshot = replay_snapshot(
            &campaign,
            &entries.levels[index],
            entries.levels.len(),
            &oracle.segments[index].replay.directions[..36],
        )
        .expect("Wobblecliff snapshot");

        assert_eq!(snapshot.level.expect("level").title, "Wobblecliff");
        assert!(snapshot.player.is_some());
        assert!(snapshot.tiles.len() > 50);
        assert!(
            snapshot
                .tiles
                .iter()
                .map(|tile| tile.source_id)
                .collect::<HashSet<_>>()
                .len()
                > 1
        );
        assert!(snapshot.tiles.iter().all(|tile| tile.pos.z > -10));
    }
}
