//! Shared three-dimensional world representation for the compatibility engine.
//!
//! It keeps every original entity, expands island masks into their real
//! voxels, and evaluates collisions using the game's timed [`Occupancy`]
//! model.

use std::collections::{HashMap, HashSet};

use crate::campaign::{Campaign, IslandMask};
use crate::physics::{Fraction, Movement, MovementKind, Occupancy, occupancy_for};
use crate::{Coord, Direction, Entity, EntityType, GameState};

#[derive(Clone, Debug, Eq, PartialEq)]
struct WorldSnapshot {
    entities: Vec<Entity>,
    movements: HashMap<i32, Movement>,
    fork_id: Option<i32>,
}

/// Authoritative mutable state shared by the general 3D rules.
#[derive(Debug)]
pub struct PhysicsWorld<'a> {
    pub campaign: &'a Campaign,
    pub entities: Vec<Entity>,
    movements: HashMap<i32, Movement>,
    entity_indices: HashMap<i32, usize>,
    player_id: i32,
    fork_id: Option<i32>,
    tile_set: i32,
    backups: Vec<WorldSnapshot>,
}

impl<'a> PhysicsWorld<'a> {
    pub fn from_state(campaign: &'a Campaign, state: &GameState) -> Result<Self, String> {
        let player_id = state
            .player()
            .ok_or_else(|| "state has no player".to_owned())?
            .id;
        let fork_id = state.fork().map(|fork| fork.id);
        let mut entity_indices = HashMap::with_capacity(state.entities.len());
        for (index, entity) in state.entities.iter().enumerate() {
            if entity_indices.insert(entity.id, index).is_some() {
                return Err(format!("state contains duplicate entity id {}", entity.id));
            }
        }
        Ok(Self {
            campaign,
            entities: state.entities.clone(),
            movements: HashMap::new(),
            entity_indices,
            player_id,
            fork_id,
            tile_set: state.tile_set,
            backups: Vec::new(),
        })
    }

    pub fn player_id(&self) -> i32 {
        self.player_id
    }

    pub fn fork_id(&self) -> Option<i32> {
        self.fork_id
    }

    pub fn player_has_fork(&self) -> bool {
        self.fork_id.is_none()
    }

    pub fn insert_fork(&mut self, mut fork: Entity) -> Result<i32, String> {
        if self.fork_id.is_some() {
            return Err("world already contains a detached fork".to_owned());
        }
        if fork.id < 0 {
            fork.id = self
                .entities
                .iter()
                .map(|entity| entity.id)
                .max()
                .unwrap_or(0)
                + 1;
        }
        if self.entity_indices.contains_key(&fork.id) {
            return Err(format!("duplicate fork entity id {}", fork.id));
        }
        let id = fork.id;
        self.entity_indices.insert(id, self.entities.len());
        self.entities.push(fork);
        self.fork_id = Some(id);
        Ok(id)
    }

    pub fn remove_fork(&mut self) -> Result<Entity, String> {
        let id = self
            .fork_id
            .take()
            .ok_or_else(|| "world has no detached fork".to_owned())?;
        self.movements.remove(&id);
        let index = self
            .entity_indices
            .remove(&id)
            .ok_or_else(|| format!("fork {id} has no entity index"))?;
        let fork = self.entities.swap_remove(index);
        if let Some(swapped) = self.entities.get(index) {
            self.entity_indices.insert(swapped.id, index);
        }
        Ok(fork)
    }

    pub fn entity(&self, id: i32) -> Option<&Entity> {
        self.entity_indices
            .get(&id)
            .and_then(|index| self.entities.get(*index))
    }

    pub fn entity_mut(&mut self, id: i32) -> Option<&mut Entity> {
        let index = *self.entity_indices.get(&id)?;
        self.entities.get_mut(index)
    }

    pub fn movement(&self, id: i32) -> Option<Movement> {
        self.movements.get(&id).copied()
    }

    pub fn movement_mut(&mut self, id: i32) -> Option<&mut Movement> {
        self.movements.get_mut(&id)
    }

    pub fn moving(&self) -> bool {
        !self.movements.is_empty()
    }

    pub fn dynamic_entity_ids(&self) -> impl Iterator<Item = i32> + '_ {
        self.entities
            .iter()
            .filter(|entity| !entity.entity_type.is_static())
            .map(|entity| entity.id)
    }

    pub fn source_footprint(&self, id: i32) -> Result<Vec<Coord>, String> {
        let entity = self
            .entity(id)
            .ok_or_else(|| format!("unknown entity id {id}"))?;
        if entity.entity_type != EntityType::Island {
            return Ok(entity.footprint(self.player_has_fork()));
        }
        let mask = self.island_mask(entity)?;
        Ok(island_cells(mask, entity.cook_data != 0)
            .map(|local| entity.pos + mask.offset + local)
            .collect())
    }

    pub fn lower_footprint(&self, id: i32) -> Result<Vec<Coord>, String> {
        let entity = self
            .entity(id)
            .ok_or_else(|| format!("unknown entity id {id}"))?;
        if entity.entity_type != EntityType::Island {
            return Ok(entity.lower_footprint(self.player_has_fork()));
        }
        let mask = self.island_mask(entity)?;
        let completed = entity.cook_data != 0;
        Ok(island_cells(mask, completed)
            .filter(|local| {
                local.z == 0
                    || !island_cell_is_solid(
                        mask.get(*local - Direction::Up.delta()).unwrap_or(0),
                        completed,
                    )
            })
            .map(|local| entity.pos + mask.offset + local + Direction::Down)
            .collect())
    }

    /// Axis-aligned bounds of the entity's stable source footprint. Island
    /// bounds come directly from mask dimensions, so broad-phase force tests
    /// do not need to expand every voxel.
    pub fn footprint_bounds(&self, id: i32) -> Result<(Coord, Coord), String> {
        let entity = self
            .entity(id)
            .ok_or_else(|| format!("unknown entity id {id}"))?;
        if entity.entity_type == EntityType::Island {
            let mask = self.island_mask(entity)?;
            let [width, height, depth] = mask.dimensions;
            let min = entity.pos + mask.offset;
            let max = Coord::new(
                min.x + width.saturating_sub(1) as i32,
                min.y + height.saturating_sub(1) as i32,
                min.z + depth.saturating_sub(1) as i32,
            );
            return Ok((min, max));
        }
        let mut min = entity.pos;
        let mut max = entity.pos;
        if self.is_extended(entity) {
            let endpoint = entity.pos + entity.direction;
            min = Coord::new(
                min.x.min(endpoint.x),
                min.y.min(endpoint.y),
                min.z.min(endpoint.z),
            );
            max = Coord::new(
                max.x.max(endpoint.x),
                max.y.max(endpoint.y),
                max.z.max(endpoint.z),
            );
        }
        Ok((min, max))
    }

    pub fn island_mask_value(&self, id: i32, world: Coord) -> Result<(i32, Coord), String> {
        let entity = self
            .entity(id)
            .ok_or_else(|| format!("unknown entity id {id}"))?;
        if entity.entity_type != EntityType::Island {
            return Err(format!("entity {id} is not an island"));
        }
        let mask = self.island_mask(entity)?;
        let local = world - entity.pos - mask.offset;
        Ok((mask.get(local).unwrap_or(0), local))
    }

    pub fn border(&self, id: i32, direction: Direction) -> Result<Vec<Coord>, String> {
        if direction.is_diagonal() || direction == Direction::None {
            return Err(format!("invalid border direction {direction:?}"));
        }
        let mut border = self.direct_border(id, direction)?;
        let entity = self
            .entity(id)
            .ok_or_else(|| format!("unknown entity id {id}"))?;
        if let Some(target) = self
            .entity(entity.stuck_to)
            .filter(|target| self.is_extended(target))
        {
            border.extend(self.direct_border(target.id, direction)?);
        }
        border.sort_by_key(|coord| (coord.x, coord.y, coord.z));
        border.dedup();
        Ok(border)
    }

    fn direct_border(&self, id: i32, direction: Direction) -> Result<Vec<Coord>, String> {
        let footprint = self.source_footprint(id)?;
        let occupied = footprint.iter().copied().collect::<HashSet<_>>();
        Ok(footprint
            .into_iter()
            .map(|pos| pos + direction)
            .filter(|pos| !occupied.contains(pos))
            .collect())
    }

    /// Matches `Entity.At`: current cells are always included; destination
    /// cells become visible after the first movement instant.
    pub fn at(&self, id: i32, pos: Coord, instant: bool) -> Result<bool, String> {
        if self.source_contains(id, pos)? {
            return Ok(true);
        }
        let Some(movement) = self.movement(id) else {
            return Ok(false);
        };
        if (instant && movement.starting()) || movement.direction == Direction::None {
            return Ok(false);
        }
        match movement.kind {
            MovementKind::Translation => self.source_contains(id, pos - movement.direction.delta()),
            MovementKind::Rotation | MovementKind::Pivot => {
                let entity = self
                    .entity(id)
                    .ok_or_else(|| format!("unknown entity id {id}"))?;
                Ok(entity.pos + movement.to == pos)
            }
            MovementKind::None => Ok(false),
        }
    }

    fn source_contains(&self, id: i32, pos: Coord) -> Result<bool, String> {
        let entity = self
            .entity(id)
            .ok_or_else(|| format!("unknown entity id {id}"))?;
        if entity.entity_type != EntityType::Island {
            return Ok(entity.footprint(self.player_has_fork()).contains(&pos));
        }
        let mask = self.island_mask(entity)?;
        let local = pos - entity.pos - mask.offset;
        Ok(island_cell_is_solid(
            mask.get(local).unwrap_or(0),
            entity.cook_data != 0,
        ))
    }

    pub fn entities_at(&self, pos: Coord, instant: bool) -> Result<Vec<i32>, String> {
        self.entities
            .iter()
            .filter(|entity| !self.is_decoration(entity))
            .filter_map(|entity| match self.at(entity.id, pos, instant) {
                Ok(true) => Some(Ok(entity.id)),
                Ok(false) => None,
                Err(error) => Some(Err(error)),
            })
            .collect()
    }

    pub fn occupancy(&self, id: i32) -> Result<Vec<Occupancy>, String> {
        let entity = self
            .entity(id)
            .ok_or_else(|| format!("unknown entity id {id}"))?;
        let movement = self.movement(id);
        if entity.entity_type != EntityType::Island {
            return Ok(occupancy_for(entity, movement, self.player_has_fork()));
        }
        let footprint = self.source_footprint(id)?;
        let Some(movement) = movement.filter(|movement| movement.kind == MovementKind::Translation)
        else {
            return Ok(footprint
                .into_iter()
                .map(|pos| Occupancy::new(pos, Direction::None, true, Fraction::ZERO, 0))
                .collect());
        };
        Ok(translation_occupancy(&footprint, movement))
    }

    pub fn mock_translation_occupancy(
        &self,
        id: i32,
        direction: Direction,
        speed: i32,
    ) -> Result<Vec<Occupancy>, String> {
        let entity = self
            .entity(id)
            .ok_or_else(|| format!("unknown entity id {id}"))?;
        if self.movement(id).is_some() {
            return Err(format!("entity {id} is already moving"));
        }
        let movement =
            Movement::translation(entity, direction, 0, speed, crate::Animation::Idle, true, 0);
        if entity.entity_type == EntityType::Island {
            Ok(translation_occupancy(&self.source_footprint(id)?, movement))
        } else {
            Ok(occupancy_for(
                entity,
                Some(movement),
                self.player_has_fork(),
            ))
        }
    }

    pub fn could_translate(
        &self,
        id: i32,
        direction: Direction,
        speed: i32,
    ) -> Result<bool, String> {
        let proposed = self.mock_translation_occupancy(id, direction, speed)?;
        self.occupancy_is_clear(id, &proposed, Some((direction, speed)), false)
    }

    pub fn collides(&self, id: i32, ignore_fork: bool) -> Result<bool, String> {
        let movement = self
            .movement(id)
            .ok_or_else(|| format!("entity {id} has no movement to collision-test"))?;
        if self
            .entity(id)
            .is_some_and(|entity| entity.entity_type == EntityType::Fork)
            && self
                .entity(id)
                .is_some_and(|fork| fork.stuck_to == self.player_id)
        {
            return Ok(false);
        }
        let occupancy = self.occupancy(id)?;
        if !self.occupancy_is_clear(
            id,
            &occupancy,
            Some((movement.direction, movement.speed)),
            ignore_fork,
        )? {
            return Ok(true);
        }
        let entity = self
            .entity(id)
            .ok_or_else(|| format!("unknown entity id {id}"))?;
        if entity.stuck_to >= 0
            && let Some(attached_movement) = self.movement(entity.stuck_to)
        {
            let attached_occupancy = self.occupancy(entity.stuck_to)?;
            return Ok(!self.occupancy_is_clear(
                entity.stuck_to,
                &attached_occupancy,
                Some((attached_movement.direction, attached_movement.speed)),
                ignore_fork,
            )?);
        }
        Ok(false)
    }

    fn occupancy_is_clear(
        &self,
        id: i32,
        occupancy: &[Occupancy],
        same_motion: Option<(Direction, i32)>,
        ignore_fork: bool,
    ) -> Result<bool, String> {
        let entity = self
            .entity(id)
            .ok_or_else(|| format!("unknown entity id {id}"))?;
        let Some((occupancy_min, occupancy_max)) = occupancy_bounds(occupancy) else {
            return Ok(true);
        };
        for other in &self.entities {
            if other.id == id
                || other.stuck_to == id
                || (ignore_fork && other.entity_type == EntityType::Fork)
                || self.is_decoration(other)
                || same_motion.is_some_and(|(direction, speed)| {
                    self.movement(other.id).is_some_and(|movement| {
                        movement.kind == MovementKind::Translation
                            && movement.direction == direction
                            && movement.speed == speed
                    })
                })
                || self.island_projection_compatible(entity, other)
            {
                continue;
            }
            let (mut other_min, mut other_max) = self.footprint_bounds(other.id)?;
            if let Some(movement) = self.movement(other.id) {
                for point in [
                    other_min + movement.direction,
                    other_max + movement.direction,
                    other.pos + movement.to,
                ] {
                    other_min = Coord::new(
                        other_min.x.min(point.x),
                        other_min.y.min(point.y),
                        other_min.z.min(point.z),
                    );
                    other_max = Coord::new(
                        other_max.x.max(point.x),
                        other_max.y.max(point.y),
                        other_max.z.max(point.z),
                    );
                }
            }
            if !bounds_overlap(occupancy_min, occupancy_max, other_min, other_max) {
                continue;
            }
            let other_occupancy = self.occupancy(other.id)?;
            if occupancy
                .iter()
                .any(|left| other_occupancy.iter().any(|right| left.overlaps(*right)))
            {
                return Ok(false);
            }
        }
        Ok(true)
    }

    pub fn set_movement(&mut self, movement: Movement) -> Result<(), String> {
        if self.entity(movement.target_id).is_none() {
            return Err(format!(
                "movement targets unknown entity {}",
                movement.target_id
            ));
        }
        if self.movements.contains_key(&movement.target_id) {
            return Err(format!("entity {} is already moving", movement.target_id));
        }
        self.movements.insert(movement.target_id, movement);
        Ok(())
    }

    pub fn clear_movement(&mut self, id: i32) -> Option<Movement> {
        self.movements.remove(&id)
    }

    pub fn next_tick_length(&self) -> Fraction {
        self.movements
            .values()
            .map(|movement| movement.remaining)
            .min()
            .unwrap_or(Fraction::ZERO)
    }

    /// Advances exactly to the next movement boundary and resolves every
    /// movement that finishes there.
    pub fn tick(&mut self) -> Result<Vec<i32>, String> {
        let elapsed = self.next_tick_length();
        if elapsed == Fraction::ZERO {
            return Ok(Vec::new());
        }
        for movement in self.movements.values_mut() {
            movement.tick(elapsed);
        }
        let mut due = self
            .movements
            .iter()
            .filter_map(|(id, movement)| movement.done().then_some(*id))
            .collect::<Vec<_>>();
        due.sort_unstable();
        let mut finished = Vec::with_capacity(due.len());
        for id in due {
            let movement = self
                .movements
                .get(&id)
                .copied()
                .expect("finished movement still exists");
            let player_turning = self
                .entity(self.player_id)
                .is_some_and(|player| player.direction.is_diagonal())
                || self
                    .movement(self.player_id)
                    .is_some_and(|movement| movement.kind == MovementKind::Rotation);
            if movement.kind == MovementKind::None && player_turning {
                self.movements
                    .get_mut(&id)
                    .expect("fixed movement still exists")
                    .set_speed(movement.speed);
                continue;
            }
            let movement = self
                .movements
                .remove(&id)
                .expect("finished movement still exists");
            let target_before = self
                .entity(id)
                .ok_or_else(|| format!("movement target {id} disappeared"))?
                .clone();
            let attached_fork = (target_before.entity_type == EntityType::Sausage)
                .then_some(target_before.stuck_to)
                .filter(|fork_id| {
                    self.entity(*fork_id)
                        .is_some_and(|entity| entity.entity_type == EntityType::Fork)
                });
            movement.resolve(
                self.entity_mut(id)
                    .expect("movement target still exists while resolving"),
            );
            if let Some(fork_id) = attached_fork
                && matches!(movement.kind, MovementKind::Rotation | MovementKind::Pivot)
            {
                let fork = self
                    .entity_mut(fork_id)
                    .ok_or_else(|| format!("attached fork {fork_id} disappeared"))?;
                fork.direction = if target_before.direction.left_of(movement.to) {
                    fork.direction.counterclockwise_45()
                } else {
                    fork.direction.clockwise_45()
                };
                if fork.pos != target_before.pos {
                    fork.pos = target_before.pos + movement.to;
                }
                if movement.kind == MovementKind::Pivot {
                    fork.pos = fork.pos + movement.direction;
                }
            }
            finished.push(id);
        }
        Ok(finished)
    }

    pub fn backup(&mut self) {
        self.backups.push(WorldSnapshot {
            entities: self.entities.clone(),
            movements: self.movements.clone(),
            fork_id: self.fork_id,
        });
    }

    pub fn restore(&mut self) -> Result<(), String> {
        let snapshot = self
            .backups
            .pop()
            .ok_or_else(|| "world backup stack is empty".to_owned())?;
        self.entities = snapshot.entities;
        self.movements = snapshot.movements;
        self.fork_id = snapshot.fork_id;
        self.entity_indices = self
            .entities
            .iter()
            .enumerate()
            .map(|(index, entity)| (entity.id, index))
            .collect();
        Ok(())
    }

    pub fn discard_backup(&mut self) -> Result<(), String> {
        self.backups
            .pop()
            .map(|_| ())
            .ok_or_else(|| "world backup stack is empty".to_owned())
    }

    fn island_mask(&self, entity: &Entity) -> Result<&IslandMask, String> {
        self.campaign
            .island_masks
            .get(&entity.data)
            .ok_or_else(|| format!("island {} has no mask {:?}", entity.id, entity.data))
    }

    fn is_extended(&self, entity: &Entity) -> bool {
        entity.entity_type.is_extended()
            || (entity.entity_type == EntityType::Player && self.player_has_fork())
    }

    fn is_decoration(&self, entity: &Entity) -> bool {
        if entity.entity_type != EntityType::Ground {
            return false;
        }
        if self.tile_set == 4
            && ((matches!(entity.tile_set, 7..=9) && (4..=7).contains(&entity.tile_number))
                || (entity.tile_set == 9 && entity.tile_number < 4))
        {
            return false;
        }
        if self.tile_set == 0 && matches!(entity.tile_set, 5 | 13) && entity.tile_number >= 6 {
            return true;
        }
        matches!(entity.tile_set, 7..=9)
    }

    fn island_projection_compatible(&self, left: &Entity, right: &Entity) -> bool {
        if left.entity_type != EntityType::Island || right.entity_type != EntityType::Island {
            return false;
        }
        let left_vertical = self
            .movement(left.id)
            .is_none_or(|movement| movement.direction.parallel_to(Direction::Up));
        let right_vertical = self
            .movement(right.id)
            .is_none_or(|movement| movement.direction.parallel_to(Direction::Up));
        if !left_vertical || !right_vertical {
            return false;
        }
        let Some(left_mask) = self.campaign.island_masks.get(&left.data) else {
            return false;
        };
        let Some(right_mask) = self.campaign.island_masks.get(&right.data) else {
            return false;
        };
        let Some(compatibility) = self
            .campaign
            .projection_compatibilities
            .get(&left.data)
            .and_then(|inner| inner.get(&right.data))
        else {
            return false;
        };
        let offset_x = left.pos.x + left_mask.offset.x - right.pos.x - right_mask.offset.x;
        let offset_y = left.pos.y + left_mask.offset.y - right.pos.y - right_mask.offset.y;
        let index_x = offset_x + left_mask.dimensions[0] as i32 - 1;
        let index_y = offset_y + left_mask.dimensions[1] as i32 - 1;
        let Ok(index_x) = usize::try_from(index_x) else {
            return true;
        };
        let Ok(index_y) = usize::try_from(index_y) else {
            return true;
        };
        if index_x >= compatibility.dimensions[0] || index_y >= compatibility.dimensions[1] {
            return true;
        }
        compatibility
            .cells
            .get(index_y + compatibility.dimensions[1] * index_x)
            .copied()
            .unwrap_or(true)
    }
}

fn island_cell_is_solid(value: i32, completed: bool) -> bool {
    value > 0 || (!completed && value == -1)
}

fn island_cells(mask: &IslandMask, completed: bool) -> impl Iterator<Item = Coord> + '_ {
    let [width, height, depth] = mask.dimensions;
    (0..width).flat_map(move |x| {
        (0..height).flat_map(move |y| {
            (0..depth).filter_map(move |z| {
                let local = Coord::new(x as i32, y as i32, z as i32);
                island_cell_is_solid(mask.get(local).unwrap_or(0), completed).then_some(local)
            })
        })
    })
}

fn translation_occupancy(footprint: &[Coord], movement: Movement) -> Vec<Occupancy> {
    let occupied = footprint.iter().copied().collect::<HashSet<_>>();
    let position = movement.position();
    let mut result = Vec::with_capacity(footprint.len() * 2);
    for pos in footprint {
        if occupied.contains(&(*pos - movement.direction.delta())) {
            result.push(Occupancy::new(
                *pos,
                Direction::None,
                true,
                Fraction::ZERO,
                0,
            ));
        } else {
            result.push(Occupancy::new(
                *pos,
                movement.direction,
                false,
                position,
                movement.speed,
            ));
        }
        if !occupied.contains(&(*pos + movement.direction)) {
            result.push(Occupancy::new(
                *pos + movement.direction,
                movement.direction,
                true,
                position,
                movement.speed,
            ));
        }
    }
    result
}

fn occupancy_bounds(occupancy: &[Occupancy]) -> Option<(Coord, Coord)> {
    let first = occupancy.first()?.pos;
    let mut min = first;
    let mut max = first;
    for item in occupancy.iter().skip(1) {
        min = Coord::new(
            min.x.min(item.pos.x),
            min.y.min(item.pos.y),
            min.z.min(item.pos.z),
        );
        max = Coord::new(
            max.x.max(item.pos.x),
            max.y.max(item.pos.y),
            max.z.max(item.pos.z),
        );
    }
    Some((min, max))
}

fn bounds_overlap(left_min: Coord, left_max: Coord, right_min: Coord, right_max: Coord) -> bool {
    left_min.x <= right_max.x
        && left_max.x >= right_min.x
        && left_min.y <= right_max.y
        && left_max.y >= right_min.y
        && left_min.z <= right_max.z
        && left_max.z >= right_min.z
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::OracleCampaign;

    fn campaign() -> Campaign {
        let root = crate::data_root();
        Campaign::load_gzip(root.join("campaign").join("merged_binary.gz"))
            .expect("campaign should parse")
    }

    fn jenga_state() -> (Campaign, GameState) {
        let campaign = campaign();
        let root = crate::data_root();
        let oracle = OracleCampaign::load(root.join("oracle").join("segments.tar.gz"))
            .expect("oracle should parse");
        let segment = oracle
            .segments
            .iter()
            .find(|segment| segment.id == "jenga3")
            .expect("jenga3 segment");
        (campaign, segment.entry.clone())
    }

    #[test]
    fn active_island_footprint_uses_original_mask_voxels() {
        let (campaign, state) = jenga_state();
        let world = PhysicsWorld::from_state(&campaign, &state).expect("3D world");
        let island = state
            .entities
            .iter()
            .find(|entity| entity.entity_type == EntityType::Island && entity.data == "jenga3")
            .expect("active island");
        let mask = campaign.island_masks.get("jenga3").expect("jenga3 mask");
        let expected = mask
            .cells
            .iter()
            .filter(|value| **value > 0 || (island.cook_data == 0 && **value == -1))
            .count();
        assert_eq!(
            world.source_footprint(island.id).expect("footprint").len(),
            expected
        );
    }

    #[test]
    fn island_translation_keeps_interior_voxels_static() {
        let (campaign, state) = jenga_state();
        let world = PhysicsWorld::from_state(&campaign, &state).expect("3D world");
        let island = state
            .entities
            .iter()
            .find(|entity| entity.entity_type == EntityType::Island && entity.data == "jenga3")
            .expect("active island");
        let footprint = world.source_footprint(island.id).expect("footprint");
        let occupancy = world
            .mock_translation_occupancy(island.id, Direction::East, 1)
            .expect("mock occupancy");
        assert!(occupancy.len() < footprint.len() * 2);
        assert!(occupancy.iter().any(|item| item.is_static()));
        assert!(
            occupancy
                .iter()
                .any(|item| item.entering && !item.is_static())
        );
        assert!(
            occupancy
                .iter()
                .any(|item| !item.entering && !item.is_static())
        );
    }

    #[test]
    fn movement_tick_resolves_all_entities_at_the_same_boundary() {
        let (campaign, state) = jenga_state();
        let mut world = PhysicsWorld::from_state(&campaign, &state).expect("3D world");
        let ids = world
            .entities
            .iter()
            .filter(|entity| entity.entity_type == EntityType::Sausage)
            .take(2)
            .map(|entity| entity.id)
            .collect::<Vec<_>>();
        assert_eq!(ids.len(), 2);
        for id in &ids {
            let entity = world.entity(*id).expect("sausage").clone();
            world
                .set_movement(Movement::translation(
                    &entity,
                    Direction::North,
                    0,
                    1,
                    crate::Animation::Idle,
                    true,
                    0,
                ))
                .expect("movement");
        }
        let before = ids
            .iter()
            .map(|id| world.entity(*id).expect("sausage").pos)
            .collect::<Vec<_>>();
        let mut finished = world.tick().expect("tick");
        finished.sort_unstable();
        let mut expected = ids.clone();
        expected.sort_unstable();
        assert_eq!(finished, expected);
        for (id, old_pos) in ids.iter().zip(before) {
            assert_eq!(
                world.entity(*id).expect("sausage").pos,
                old_pos + Direction::North
            );
        }
    }

    #[test]
    fn nested_backups_restore_entities_and_movements() {
        let (campaign, state) = jenga_state();
        let mut world = PhysicsWorld::from_state(&campaign, &state).expect("3D world");
        let player_id = world.player_id();
        let original = world.entity(player_id).expect("player").clone();
        world.backup();
        let movement = Movement::translation(
            &original,
            Direction::North,
            0,
            1,
            crate::Animation::WalkForward,
            true,
            0,
        );
        world.set_movement(movement).expect("movement");
        world.tick().expect("tick");
        assert_ne!(world.entity(player_id), Some(&original));
        world.restore().expect("restore");
        assert_eq!(world.entity(player_id), Some(&original));
        assert!(!world.moving());
    }
}
