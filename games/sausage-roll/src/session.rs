use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::{
    Campaign, CampaignEntries, Coord, Direction, Entity, EntityType, Game3d, PhysicsWorld,
};

pub const CAMPAIGN_ID: &str = "stephens-sausage-roll-complete";

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SessionRecord {
    pub schema: String,
    pub solved: usize,
    pub histories: Vec<Vec<Direction>>,
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
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct EntityView {
    pub id: i32,
    pub kind: EntityType,
    pub pos: Coord,
    pub direction: Direction,
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
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct GameSnapshot {
    pub schema: &'static str,
    pub campaign: CampaignStatus,
    pub status: String,
    pub level: Option<LevelStatus>,
    pub exit_ready: bool,
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
    pub solved_levels: Vec<String>,
    pub snapshot: GameSnapshot,
}

pub struct Session<'a> {
    campaign: &'a Campaign,
    entries: &'a CampaignEntries,
    histories: Vec<Vec<Direction>>,
    solved: usize,
    game: Option<Game3d<'a>>,
}

impl<'a> Session<'a> {
    pub fn new(campaign: &'a Campaign, entries: &'a CampaignEntries) -> Result<Self, String> {
        entries.validate_against(campaign)?;
        let histories = vec![Vec::new(); entries.levels.len()];
        let game = entries
            .levels
            .first()
            .map(|entry| Game3d::from_state(campaign, &entry.state))
            .transpose()?;
        Ok(Self {
            campaign,
            entries,
            histories,
            solved: 0,
            game,
        })
    }

    pub fn restore(
        campaign: &'a Campaign,
        entries: &'a CampaignEntries,
        record: SessionRecord,
    ) -> Result<Self, String> {
        if record.schema != "sausage-session-v1" {
            return Err(format!("unknown session schema {:?}", record.schema));
        }
        if record.histories.len() != entries.levels.len() || record.solved > entries.levels.len() {
            return Err("saved session has invalid campaign dimensions".to_owned());
        }
        let mut session = Self {
            campaign,
            entries,
            histories: record.histories,
            solved: record.solved,
            game: None,
        };
        for index in 0..session.solved {
            let game = session.replay_level(index)?;
            if !game.can_exit() {
                return Err(format!(
                    "saved history for level {} is not solved at its exit",
                    index + 1
                ));
            }
        }
        if session.solved < entries.levels.len() {
            session.game = Some(session.replay_level(session.solved)?);
        }
        Ok(session)
    }

    pub fn record(&self) -> SessionRecord {
        SessionRecord {
            schema: "sausage-session-v1".to_owned(),
            solved: self.solved,
            histories: self.histories.clone(),
        }
    }

    pub fn snapshot(&self) -> Result<GameSnapshot, String> {
        let total = self.entries.levels.len();
        let campaign = CampaignStatus {
            id: CAMPAIGN_ID,
            score: self.solved,
            solved: self.solved,
            total,
            complete: self.solved == total,
        };
        let Some(game) = &self.game else {
            return Ok(GameSnapshot {
                schema: "sausage-state-v1",
                campaign,
                status: "complete".to_owned(),
                level: None,
                exit_ready: false,
                player: None,
                entities: Vec::new(),
                tiles: Vec::new(),
                controls: ["north", "south", "west", "east"],
            });
        };
        let entry = &self.entries.levels[self.solved];
        let level = LevelStatus {
            ordinal: entry.ordinal,
            id: entry.id.clone(),
            title: entry.state.display_name.clone(),
            status: if game.won() { "cooked" } else { "in_progress" }.to_owned(),
            actions: self.histories[self.solved].len(),
        };
        let (player, entities, tiles) = project_world(&game.world)?;
        Ok(GameSnapshot {
            schema: "sausage-state-v1",
            campaign,
            status: "in_progress".to_owned(),
            level: Some(level),
            exit_ready: game.can_exit(),
            player,
            entities,
            tiles,
            controls: ["north", "south", "west", "east"],
        })
    }

    pub fn levels(&self) -> Vec<LevelStatus> {
        self.entries
            .levels
            .iter()
            .enumerate()
            .map(|(index, entry)| LevelStatus {
                ordinal: entry.ordinal,
                id: entry.id.clone(),
                title: entry.state.display_name.clone(),
                status: if index < self.solved {
                    "solved"
                } else if index == self.solved {
                    "current"
                } else {
                    "locked"
                }
                .to_owned(),
                actions: self.histories[index].len(),
            })
            .collect()
    }

    pub fn move_many(&mut self, directions: &[Direction]) -> Result<MoveResult, String> {
        if directions.is_empty() {
            return Err("move requires at least one direction".to_owned());
        }
        let requested = directions.len();
        let mut accepted = Vec::new();
        let mut solved_levels = Vec::new();
        for direction in directions {
            let index = self.solved;
            let game = self
                .game
                .as_mut()
                .ok_or_else(|| "campaign is already complete".to_owned())?;
            let did_move = game.step(*direction)?;
            self.histories[index].push(*direction);
            accepted.push(did_move);
            if game.can_exit() {
                solved_levels.push(self.entries.levels[index].id.clone());
                self.solved += 1;
                self.game = self
                    .entries
                    .levels
                    .get(self.solved)
                    .map(|entry| Game3d::from_state(self.campaign, &entry.state))
                    .transpose()?;
                break;
            }
        }
        Ok(MoveResult {
            requested,
            applied: accepted.len(),
            accepted,
            solved_levels,
            snapshot: self.snapshot()?,
        })
    }

    pub fn undo(&mut self, count: usize) -> Result<usize, String> {
        if count == 0 {
            return Err("undo count must be positive".to_owned());
        }
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

    pub fn restart(&mut self) -> Result<(), String> {
        if self.solved == self.entries.levels.len() {
            return Err("campaign is already complete".to_owned());
        }
        self.histories[self.solved].clear();
        self.game = Some(Game3d::from_state(
            self.campaign,
            &self.entries.levels[self.solved].state,
        )?);
        Ok(())
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

fn project_world(world: &PhysicsWorld<'_>) -> Result<WorldProjection, String> {
    let player = world
        .entity(world.player_id())
        .ok_or_else(|| "player disappeared".to_owned())?;
    let center = player.pos;
    let visible = |pos: Coord| {
        pos.z > -10 && (pos.x - center.x).abs() <= 64 && (pos.y - center.y).abs() <= 64
    };
    let mut entities = world
        .entities
        .iter()
        .filter(|entity| {
            !entity.entity_type.is_static()
                && entity.entity_type != EntityType::Island
                && visible(entity.pos)
        })
        .map(|entity| entity_view(world, entity))
        .collect::<Result<Vec<_>, _>>()?;
    entities.sort_by_key(|entity| entity.id);
    let player_view = entities
        .iter()
        .find(|entity| entity.id == world.player_id())
        .cloned();

    let mut tiles = HashMap::<Coord, TileView>::new();
    for island in world
        .entities
        .iter()
        .filter(|entity| entity.entity_type == EntityType::Island)
    {
        let (min, max) = world.footprint_bounds(island.id)?;
        if max.z <= -10
            || max.x < center.x - 64
            || min.x > center.x + 64
            || max.y < center.y - 64
            || min.y > center.y + 64
        {
            continue;
        }
        for pos in world.source_footprint(island.id)? {
            if visible(pos) {
                tiles.insert(
                    pos,
                    TileView {
                        pos,
                        kind: "land".to_owned(),
                        source_id: island.id,
                    },
                );
            }
        }
    }
    for entity in world
        .entities
        .iter()
        .filter(|entity| entity.entity_type.is_static() && visible(entity.pos))
    {
        let kind = match entity.entity_type {
            EntityType::Bbq => "grill",
            EntityType::Ladder => "ladder",
            EntityType::Ground => "ground",
            EntityType::SpectralSausage => "spectral_sausage",
            _ => "static",
        };
        tiles.insert(
            entity.pos,
            TileView {
                pos: entity.pos,
                kind: kind.to_owned(),
                source_id: entity.id,
            },
        );
    }
    let mut tiles = tiles.into_values().collect::<Vec<_>>();
    tiles.sort_by_key(|tile| (tile.pos.z, tile.pos.y, tile.pos.x, tile.source_id));
    Ok((player_view, entities, tiles))
}

fn entity_view(world: &PhysicsWorld<'_>, entity: &Entity) -> Result<EntityView, String> {
    let cooked_faces = (entity.entity_type == EntityType::Sausage).then_some({
        [
            entity.cook_data % 4,
            entity.cook_data / 4 % 4,
            entity.cook_data / 16 % 4,
            entity.cook_data / 64 % 4,
        ]
    });
    Ok(EntityView {
        id: entity.id,
        kind: entity.entity_type,
        pos: entity.pos,
        direction: entity.direction,
        cells: world.source_footprint(entity.id)?,
        attached_to: (entity.stuck_to >= 0).then_some(entity.stuck_to),
        cooked_faces,
        island: (entity.entity_type == EntityType::Island).then(|| entity.data.clone()),
    })
}

#[cfg(test)]
mod tests {
    use crate::OracleCampaign;

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
    fn first_walkthrough_level_advances_the_campaign() {
        let (campaign, entries, oracle) = inputs();
        let mut session = Session::new(&campaign, &entries).expect("session");
        let first = &oracle.segments[0];
        for direction in &first.replay.directions {
            session.move_many(&[*direction]).expect("walkthrough move");
        }
        assert_eq!(session.record().solved, 1);
        assert_eq!(
            session
                .snapshot()
                .expect("snapshot")
                .level
                .expect("level")
                .id,
            oracle.segments[1].id
        );
    }

    #[test]
    fn undo_across_a_level_boundary_reopens_the_previous_level() {
        let (campaign, entries, oracle) = inputs();
        let mut session = Session::new(&campaign, &entries).expect("session");
        session
            .move_many(&oracle.segments[0].replay.directions)
            .expect("walkthrough");
        assert_eq!(session.record().solved, 1);
        assert_eq!(session.undo(1).expect("undo"), 1);
        assert_eq!(session.record().solved, 0);
        assert_eq!(
            session
                .snapshot()
                .expect("snapshot")
                .level
                .expect("level")
                .id,
            oracle.segments[0].id
        );
    }
}
