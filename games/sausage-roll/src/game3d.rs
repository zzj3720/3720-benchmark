//! Input and force rules for the general three-dimensional engine.
//!
//! This layer deliberately operates on [`crate::PhysicsWorld`] rather than a
//! puzzle-specific grid. Walking, turning, recursive pushes, passive tower
//! forces, gravity, ladders, cooking, fork attachment, pivots, and moving
//! islands all resolve through the same state machine.

use std::collections::HashSet;

use crate::{
    Animation, Campaign, Coord, Direction, EntityType, GameState, Movement, MovementKind,
    PhysicsWorld,
};

#[derive(Clone, Copy, Debug)]
struct PassiveForce {
    direction: Direction,
    speed: i32,
    torsion: i32,
}

#[derive(Debug)]
pub struct Game3d<'a> {
    pub world: PhysicsWorld<'a>,
    overworld: bool,
    push_target_level: String,
    exit_pos: Coord,
    exit_direction: Direction,
    exit_up: bool,
    exit_attachment: Option<i32>,
    last_direction: Direction,
}

impl<'a> Game3d<'a> {
    pub fn from_state(campaign: &'a Campaign, state: &GameState) -> Result<Self, String> {
        state
            .player()
            .ok_or_else(|| "state has no player".to_owned())?;
        let entry = campaign
            .player_positions
            .get(&state.push_target_level)
            .ok_or_else(|| {
                format!(
                    "puzzle {:?} has no campaign entry position",
                    state.push_target_level
                )
            })?;
        let active_island = state
            .entities
            .iter()
            .find(|entity| {
                entity.entity_type == EntityType::Island && entity.data == state.push_target_level
            })
            .ok_or_else(|| format!("puzzle {:?} has no active island", state.push_target_level))?;
        let exit_pos = entry.pos + active_island.pos;
        let player_has_fork = state.fork().is_none();
        let exit_attachment = state
            .entities
            .iter()
            .find(|entity| {
                entity.entity_type == EntityType::Sausage
                    && entity
                        .footprint(player_has_fork)
                        .contains(&(exit_pos + Direction::Down))
            })
            .map(|entity| entity.id);
        Ok(Self {
            world: PhysicsWorld::from_state(campaign, state)?,
            overworld: state.overworld,
            push_target_level: state.push_target_level.clone(),
            exit_pos,
            exit_direction: entry.direction,
            exit_up: true,
            exit_attachment,
            last_direction: Direction::None,
        })
    }

    pub fn won(&self) -> bool {
        let mut found = false;
        for sausage in self
            .world
            .entities
            .iter()
            .filter(|entity| entity.entity_type == EntityType::Sausage)
            .filter(|entity| entity.pos.z <= 8 && !entity.data.starts_with('S'))
        {
            if sausage.pos.z < -3 {
                return false;
            }
            found = true;
            let mut faces = sausage.cook_data;
            for _ in 0..4 {
                if matches!(faces % 4, 0 | 3) {
                    return false;
                }
                faces /= 4;
            }
        }
        found
    }

    pub fn complete(&self) -> bool {
        self.won()
    }

    pub fn exit(&self) -> (Coord, Direction) {
        (self.exit_pos, self.exit_direction)
    }

    pub fn can_exit(&self) -> bool {
        if !self.won() || !self.world.player_has_fork() || !self.exit_up {
            return false;
        }
        self.world
            .entity(self.world.player_id())
            .is_some_and(|player| {
                player.pos == self.exit_pos && player.direction == self.exit_direction
            })
    }

    pub fn step(&mut self, direction: Direction) -> Result<bool, String> {
        if !direction.is_cardinal() {
            return Err(format!(
                "input direction must be cardinal, got {direction:?}"
            ));
        }
        if self.world.movement(self.world.player_id()).is_some() {
            return Ok(false);
        }
        let player = self
            .world
            .entity(self.world.player_id())
            .ok_or_else(|| "player disappeared".to_owned())?
            .clone();
        if player.pos.z < -2 {
            return Ok(false);
        }
        let laden = player.stuck_to >= 0;
        let extended = self.world.player_has_fork();
        let parallel = direction.parallel_to(player.direction);
        let accepted = if laden {
            if parallel {
                self.try_move_player(direction)?
            } else if self.ladder_up_in_direction(direction)? {
                self.try_climb_up(direction)?
            } else if self.ladder_down_in_direction(direction)? {
                self.try_climb_down(direction)?
            } else {
                self.try_move_player(direction)?
            }
        } else if parallel {
            if !extended
                && direction == player.direction
                && self.ladder_up_in_direction(direction)?
            {
                self.try_climb_up(direction)?
            } else if !extended
                && direction == player.direction.inverse()
                && self.ladder_down_in_direction(direction)?
            {
                self.try_climb_down(direction)?
            } else {
                self.try_move_player(direction)?
            }
        } else if extended && self.ladder_up_in_direction(direction)? {
            self.try_climb_up(direction)?
        } else if extended && self.ladder_down_in_direction(direction)? {
            self.try_climb_down(direction)?
        } else {
            self.try_turn(
                self.world.player_id(),
                player.direction.left_of(direction),
                2,
            )?
        };
        if accepted {
            self.passive_force_sweep()?;
            self.settle()?;
        }
        Ok(accepted)
    }

    pub fn settle(&mut self) -> Result<(), String> {
        let mut iterations = 0usize;
        loop {
            iterations += 1;
            if iterations > 10_000 {
                return Err("automatic movement did not settle".to_owned());
            }
            let has_active_movement = self.world.dynamic_entity_ids().any(|id| {
                self.world
                    .movement(id)
                    .is_some_and(|movement| movement.kind != MovementKind::None)
            });
            if has_active_movement {
                let moving_islands = self
                    .world
                    .dynamic_entity_ids()
                    .filter(|id| {
                        self.world
                            .entity(*id)
                            .is_some_and(|entity| entity.entity_type == EntityType::Island)
                            && self
                                .world
                                .movement(*id)
                                .is_some_and(|movement| movement.kind != MovementKind::None)
                    })
                    .collect::<HashSet<_>>();
                let finished = self.tick_world()?;
                let cook_targets = if finished.iter().any(|id| moving_islands.contains(id)) {
                    self.world.dynamic_entity_ids().collect::<Vec<_>>()
                } else {
                    finished
                };
                for id in cook_targets {
                    self.cook(id)?;
                }
                self.process_gravity()?;
                self.passive_force_sweep()?;
                self.process_gravity()?;
                continue;
            }
            let player = self
                .world
                .entity(self.world.player_id())
                .ok_or_else(|| "player disappeared".to_owned())?
                .clone();
            if player.direction.is_diagonal() {
                if !self.automatic_turn(player.id, 2)? {
                    return Err("player automatic turn reached an invalid state".to_owned());
                }
                continue;
            }
            if player
                .data
                .parse::<i32>()
                .is_ok_and(|direction| (0..=3).contains(&direction))
            {
                if !self.automatic_climb_up()? {
                    return Err("automatic climb-up reached an invalid state".to_owned());
                }
                self.passive_force_sweep()?;
                continue;
            }
            if player
                .data
                .parse::<i32>()
                .is_ok_and(|direction| (-4..=-1).contains(&direction))
            {
                if !self.automatic_climb_down()? {
                    return Err("automatic climb-down reached an invalid state".to_owned());
                }
                self.passive_force_sweep()?;
                continue;
            }
            if self.bbq_at(player.pos + Direction::Down)?.0 != Direction::None {
                let retreat = if self.last_direction.is_horizontal() {
                    self.last_direction.inverse()
                } else {
                    player.direction
                };
                if self.try_move_player(retreat)? || self.try_move_player(retreat.inverse())? {
                    self.passive_force_sweep()?;
                    continue;
                }
            }
            if self.world.moving() {
                self.tick_world()?;
                continue;
            }
            if self.process_gravity()? {
                continue;
            }
            break;
        }
        Ok(())
    }

    fn tick_world(&mut self) -> Result<Vec<i32>, String> {
        let attachment_movement = self
            .exit_attachment
            .and_then(|id| self.world.movement(id).zip(self.world.entity(id).cloned()));
        let finished = self.world.tick()?;
        if let Some((movement, entity)) = attachment_movement
            && finished.contains(&entity.id)
        {
            match movement.kind {
                MovementKind::Translation => {
                    self.exit_pos = self.exit_pos + movement.direction;
                    if movement.torsion != 0 {
                        self.exit_up = !self.exit_up;
                        if self.exit_direction.normal_to(entity.direction) {
                            self.exit_direction = self.exit_direction.inverse();
                        }
                    }
                }
                MovementKind::Rotation => {
                    self.exit_direction = if entity.direction.left_of(movement.to) {
                        self.exit_direction.counterclockwise_45()
                    } else {
                        self.exit_direction.clockwise_45()
                    };
                    if self.exit_pos != entity.pos {
                        self.exit_pos = entity.pos + movement.to + Direction::Up;
                    }
                }
                _ => {}
            }
        }
        Ok(finished)
    }

    fn try_move_player(&mut self, direction: Direction) -> Result<bool, String> {
        self.try_move_player_inner(direction, false)
    }

    fn try_move_player_inner(
        &mut self,
        direction: Direction,
        unfork: bool,
    ) -> Result<bool, String> {
        let player_id = self.world.player_id();
        let player = self
            .world
            .entity(player_id)
            .ok_or_else(|| "player disappeared".to_owned())?
            .clone();
        self.last_direction = direction;
        let Some(floor_id) = self.floor_at(player.pos, false)? else {
            return Ok(false);
        };
        let forward_floor = self.floor_at(player.pos + direction, false)?;

        self.world.backup();
        if let Some(forward_floor_id) = forward_floor
            && self
                .world
                .entity(forward_floor_id)
                .is_some_and(|entity| entity.entity_type == EntityType::Island)
            && self.ladder_at(player.pos + Direction::Down)? != direction
            && self.world.movement(forward_floor_id).is_none()
        {
            let island = self
                .world
                .entity(forward_floor_id)
                .expect("forward island exists")
                .clone();
            self.world.set_movement(Movement::fixed(&island, 1))?;
        }
        let mut movement_direction = direction;
        let mut rolled_underfoot = false;
        let floor = self
            .world
            .entity(floor_id)
            .ok_or_else(|| format!("floor entity {floor_id} disappeared"))?
            .clone();
        if floor.entity_type == EntityType::Sausage && direction.normal_to(floor.direction) {
            let _pushed = self.apply_force_at(
                player.pos + Direction::Down,
                direction.inverse(),
                1,
                1,
                Some(player_id),
            )?;
            self.passive_force_sweep()?;
            if let Some(movement) = self.world.movement_mut(floor_id)
                && movement.torsion != 1
            {
                movement.torsion = 1;
            }
            let has_torsion = self
                .world
                .movement(floor_id)
                .is_some_and(|movement| movement.torsion != 0);
            let has_stationary_support = has_torsion
                && self
                    .footprint_entities(floor_id)?
                    .into_iter()
                    .any(|support| {
                        self.world
                            .movement(support)
                            .is_none_or(|movement| movement.kind == MovementKind::None)
                    });
            if has_stationary_support {
                rolled_underfoot = true;
                movement_direction = direction.inverse();
            } else {
                self.world.restore()?;
                self.world.backup();
            }
        }
        let temporary = Movement::translation(
            &player,
            movement_direction,
            0,
            1,
            Animation::ForwardPedal,
            false,
            0,
        );
        if unfork {
            self.world.set_movement(temporary)?;
        } else {
            self.set_movement_with_attachment(temporary)
                .map_err(|error| format!("temporary player movement failed: {error}"))?;
        }
        self.apply_force_from(player_id, movement_direction, 1, 1)?;
        if unfork {
            self.world.clear_movement(player_id);
        } else {
            self.clear_movement_with_attachment(player_id);
        }

        let animation = if rolled_underfoot {
            Animation::ForwardPedal
        } else if movement_direction == player.direction.inverse() {
            Animation::Backpedal
        } else if movement_direction.normal_to(player.direction) {
            if movement_direction.left_of(player.direction) {
                Animation::StrafeLeft
            } else {
                Animation::StrafeRight
            }
        } else {
            Animation::WalkForward
        };
        let final_movement = Movement::translation(
            &player,
            movement_direction,
            0,
            1,
            animation,
            movement_direction.left_of(player.direction),
            0,
        );
        if unfork {
            self.world.set_movement(final_movement)?;
        } else {
            self.set_movement_with_attachment(final_movement)
                .map_err(|error| format!("final player movement failed: {error}"))?;
        }
        if !unfork && movement_direction == player.direction {
            self.try_fork()?;
        }

        let unstable = !rolled_underfoot
            && forward_floor.is_none_or(|id| {
                self.world
                    .movement(id)
                    .is_some_and(|movement| movement.kind != MovementKind::None)
            });
        let collision = self.world.collides(player_id, false)?;
        if unstable || collision {
            self.world.restore()?;
            if !unfork && player.stuck_to >= 0 && direction == player.direction.inverse() {
                return self.try_move_player_inner(direction, true);
            }
            return Ok(false);
        }
        if self.world.movement(floor_id).is_none() && forward_floor.is_none() {
            self.world.restore()?;
            return Ok(false);
        }
        self.world.discard_backup()?;
        if unfork && player.stuck_to >= 0 {
            self.world
                .entity_mut(player_id)
                .expect("player exists")
                .stuck_to = -1;
            self.world
                .entity_mut(player.stuck_to)
                .ok_or_else(|| format!("attached entity {} disappeared", player.stuck_to))?
                .stuck_to = -1;
        }
        Ok(true)
    }

    fn set_movement_with_attachment(&mut self, movement: Movement) -> Result<(), String> {
        let target_id = movement.target_id;
        if self.world.movement(target_id).is_some() {
            return Ok(());
        }
        let attached = self
            .world
            .entity(target_id)
            .and_then(|entity| (entity.stuck_to >= 0).then_some(entity.stuck_to));
        self.world.set_movement(movement).map_err(|error| {
            format!("could not move entity {target_id} with attachment: {error}")
        })?;
        if let Some(attached_id) = attached
            && self.world.movement(attached_id).is_none()
        {
            let attached_entity = self
                .world
                .entity(attached_id)
                .ok_or_else(|| format!("attached entity {attached_id} disappeared"))?
                .clone();
            self.world
                .set_movement(Movement::translation(
                    &attached_entity,
                    movement.direction,
                    movement.torsion,
                    movement.speed,
                    movement.animation,
                    movement.left,
                    movement.tower_level,
                ))
                .map_err(|error| {
                    format!(
                        "could not move attachment {attached_id} with entity {target_id}: {error}"
                    )
                })?;
        }
        Ok(())
    }

    fn clear_movement_with_attachment(&mut self, id: i32) {
        let attached = self
            .world
            .entity(id)
            .and_then(|entity| (entity.stuck_to >= 0).then_some(entity.stuck_to));
        self.world.clear_movement(id);
        if let Some(attached_id) = attached {
            self.world.clear_movement(attached_id);
        }
    }

    fn try_fork(&mut self) -> Result<(), String> {
        let player_id = self.world.player_id();
        let player = self
            .world
            .entity(player_id)
            .ok_or_else(|| "player disappeared".to_owned())?
            .clone();
        let Some(movement) = self.world.movement(player_id) else {
            return Ok(());
        };
        if player.stuck_to >= 0
            || !self.world.player_has_fork()
            || movement.kind != MovementKind::Translation
            || movement.direction != player.direction
        {
            return Ok(());
        }
        let player_target = player.pos + movement.direction;
        let fork_target = player_target + player.direction;
        let Some(sausage_id) = self
            .world
            .entities_at(fork_target, false)?
            .into_iter()
            .find(|id| {
                self.world.entity(*id).is_some_and(|entity| {
                    entity.entity_type == EntityType::Sausage
                        && entity.stuck_to < 0
                        && self.world.movement(*id).is_none()
                })
            })
        else {
            return Ok(());
        };
        if self.world.at(sausage_id, player_target, true)? {
            return Ok(());
        }
        self.world
            .entity_mut(player_id)
            .expect("player exists")
            .stuck_to = sausage_id;
        self.world
            .entity_mut(sausage_id)
            .expect("sausage exists")
            .stuck_to = player_id;
        Ok(())
    }

    fn try_climb_up(&mut self, direction: Direction) -> Result<bool, String> {
        let player_id = self.world.player_id();
        let player = self
            .world
            .entity(player_id)
            .ok_or_else(|| "player disappeared".to_owned())?
            .clone();
        self.world.backup();
        self.set_movement_with_attachment(Movement::translation(
            &player,
            Direction::Up,
            0,
            1,
            Animation::ClimbUpInit,
            direction.left_of(player.direction),
            0,
        ))?;
        self.apply_force_from(player_id, Direction::Up, 0, 1)?;
        self.world
            .entity_mut(player_id)
            .expect("player exists")
            .data = (direction as i32).to_string();
        if self.world.collides(player_id, false)? {
            self.world.restore()?;
            return Ok(false);
        }
        self.world.discard_backup()?;
        Ok(true)
    }

    fn automatic_climb_up(&mut self) -> Result<bool, String> {
        let player_id = self.world.player_id();
        let player = self
            .world
            .entity(player_id)
            .ok_or_else(|| "player disappeared".to_owned())?
            .clone();
        let value = player
            .data
            .parse::<i32>()
            .map_err(|error| format!("invalid climb direction {:?}: {error}", player.data))?;
        let direction = Direction::from_i32(value)?;
        let ladder_pos = player.pos + direction;
        self.world.backup();
        let (movement_direction, animation, keep_climbing) =
            if self.ladder_at(ladder_pos)? == direction.inverse() {
                let continues = self.ladder_at(ladder_pos + Direction::Up)? == direction.inverse();
                (
                    Direction::Up,
                    if continues {
                        Animation::ClimbUpLoop
                    } else {
                        Animation::ClimbUpEnd1
                    },
                    true,
                )
            } else {
                (direction, Animation::ClimbUpEnd2, false)
            };
        self.set_movement_with_attachment(Movement::translation(
            &player,
            movement_direction,
            0,
            1,
            animation,
            direction.left_of(player.direction),
            0,
        ))?;
        self.apply_force_from(player_id, movement_direction, 0, 1)?;
        if !keep_climbing {
            self.world
                .entity_mut(player_id)
                .expect("player exists")
                .data
                .clear();
        }
        if self.world.collides(player_id, false)? {
            self.world.restore()?;
            return Ok(false);
        }
        self.world.discard_backup()?;
        Ok(true)
    }

    fn try_climb_down(&mut self, direction: Direction) -> Result<bool, String> {
        let player_id = self.world.player_id();
        let player = self
            .world
            .entity(player_id)
            .ok_or_else(|| "player disappeared".to_owned())?
            .clone();
        self.world.backup();
        self.set_movement_with_attachment(Movement::translation(
            &player,
            direction,
            0,
            1,
            Animation::ClimbDownInit1,
            direction.left_of(player.direction),
            0,
        ))?;
        self.apply_force_from(player_id, direction, 1, 1)?;
        self.world
            .entity_mut(player_id)
            .expect("player exists")
            .data = (-1 - direction as i32).to_string();
        if self.world.collides(player_id, false)? {
            self.world.restore()?;
            return Ok(false);
        }
        self.world.discard_backup()?;
        Ok(true)
    }

    fn automatic_climb_down(&mut self) -> Result<bool, String> {
        let player_id = self.world.player_id();
        let player = self
            .world
            .entity(player_id)
            .ok_or_else(|| "player disappeared".to_owned())?
            .clone();
        let value = player
            .data
            .parse::<i32>()
            .map_err(|error| format!("invalid climb direction {:?}: {error}", player.data))?;
        let direction = Direction::from_i32(-value - 1)?;
        self.world.backup();
        self.set_movement_with_attachment(Movement::translation(
            &player,
            Direction::Down,
            0,
            1,
            Animation::ClimbDownLoop,
            direction.left_of(player.direction),
            0,
        ))?;
        self.apply_force_from(player_id, Direction::Down, 1, 1)?;
        if self
            .floor_at(player.pos + Direction::Down, false)?
            .is_some()
        {
            self.world
                .entity_mut(player_id)
                .expect("player exists")
                .data
                .clear();
        }
        if self.world.collides(player_id, false)? {
            self.world.restore()?;
            return Ok(false);
        }
        self.world.discard_backup()?;
        Ok(true)
    }

    fn ladder_up_in_direction(&self, direction: Direction) -> Result<bool, String> {
        let player = self
            .world
            .entity(self.world.player_id())
            .ok_or_else(|| "player disappeared".to_owned())?;
        Ok(self.ladder_at(player.pos + direction)? == direction.inverse())
    }

    fn ladder_down_in_direction(&self, direction: Direction) -> Result<bool, String> {
        let player = self
            .world
            .entity(self.world.player_id())
            .ok_or_else(|| "player disappeared".to_owned())?;
        if self.ladder_at(player.pos + Direction::Down)? != direction {
            return Ok(false);
        }
        Ok(self
            .world
            .entities_at(player.pos + direction + Direction::Down, false)?
            .is_empty())
    }

    fn ladder_at(&self, pos: Coord) -> Result<Direction, String> {
        for id in self.world.entities_at(pos, false)? {
            let entity = self
                .world
                .entity(id)
                .ok_or_else(|| format!("ladder entity {id} disappeared"))?;
            if entity.entity_type == EntityType::Ladder {
                return Ok(entity.direction);
            }
            if entity.entity_type == EntityType::Island {
                let (value, _) = self.world.island_mask_value(id, pos)?;
                if (3..=6).contains(&value) {
                    return Direction::from_i32(value - 3);
                }
            }
        }
        Ok(Direction::None)
    }

    fn try_turn(&mut self, id: i32, clockwise: bool, mut turn_speed: i32) -> Result<bool, String> {
        if self.world.movement(id).is_some() {
            return Ok(false);
        }
        let entity = self
            .world
            .entity(id)
            .ok_or_else(|| format!("turn target {id} disappeared"))?
            .clone();
        let final_direction = entity.direction.rotate_90(clockwise);
        let intermediate = Direction::rotation_between(entity.direction, final_direction)
            .ok_or_else(|| format!("cannot turn from {:?}", entity.direction))?;

        self.world.backup();
        let hat = self.get_hat(id)?;
        if hat.is_some() {
            turn_speed = 1;
        }
        self.world.set_movement(Movement::rotation(
            &entity,
            entity.direction,
            intermediate,
            Animation::TurnIn,
            1,
        ))?;
        let fixed_floor = if id == self.world.player_id() {
            let floor = self.floor_at(entity.pos, false)?;
            if let Some(floor_id) = floor.filter(|floor_id| {
                self.world
                    .entity(*floor_id)
                    .is_some_and(|floor| !floor.entity_type.is_static())
            }) {
                self.world.clear_movement(floor_id);
                let floor = self
                    .world
                    .entity(floor_id)
                    .expect("turn floor exists")
                    .clone();
                self.world
                    .set_movement(Movement::fixed(&floor, turn_speed))?;
                Some(floor_id)
            } else {
                None
            }
        } else {
            None
        };
        let forced = if self.is_extended(id)? {
            self.apply_force_at(entity.pos + intermediate, final_direction, 1, 1, Some(id))?
        } else {
            false
        };
        if forced {
            turn_speed = 1;
        }
        self.world.clear_movement(id);
        if let Some(floor_id) = fixed_floor
            && self
                .world
                .movement(floor_id)
                .is_some_and(|movement| movement.kind != MovementKind::None)
        {
            self.world
                .movement_mut(floor_id)
                .expect("moving turn floor exists")
                .set_speed(turn_speed);
        }
        let current = self
            .world
            .entity(id)
            .ok_or_else(|| format!("turn target {id} disappeared"))?
            .clone();
        self.world.set_movement(Movement::rotation(
            &current,
            current.direction,
            intermediate,
            Animation::TurnIn,
            turn_speed,
        ))?;
        self.world
            .entity_mut(id)
            .expect("turn target exists")
            .turn_direction = final_direction;
        if self.world.collides(id, false)? {
            self.world.restore()?;
            if id == self.world.player_id() {
                return self.try_pivot_turn(id, clockwise, None);
            }
            return Ok(false);
        }
        self.world.discard_backup()?;

        if let Some(hat_id) = hat {
            self.passive_force_sweep()?;
            let _ = self.try_turn(hat_id, clockwise, turn_speed)?;
        }
        Ok(true)
    }

    fn try_pivot_turn(
        &mut self,
        id: i32,
        clockwise: bool,
        requested_push: Option<Direction>,
    ) -> Result<bool, String> {
        if self.world.movement(id).is_some() {
            return Ok(false);
        }
        let entity = self
            .world
            .entity(id)
            .ok_or_else(|| format!("pivot target {id} disappeared"))?
            .clone();
        let player_pos = self
            .world
            .entity(self.world.player_id())
            .ok_or_else(|| "player disappeared during pivot".to_owned())?
            .pos;
        let Some(floor_id) = self.floor_at(player_pos, false)? else {
            return Ok(false);
        };
        if self
            .world
            .entity(floor_id)
            .is_none_or(|floor| floor.entity_type != EntityType::Island)
        {
            return Ok(false);
        }
        let final_direction = entity.direction.rotate_90(clockwise);
        let intermediate = Direction::rotation_between(entity.direction, final_direction)
            .ok_or_else(|| format!("cannot pivot from {:?}", entity.direction))?;
        let movement_direction = requested_push.unwrap_or(final_direction.inverse());
        self.world.backup();
        if id == self.world.player_id()
            && self
                .world
                .movement(floor_id)
                .is_some_and(|movement| movement.kind == MovementKind::None)
        {
            self.world.clear_movement(floor_id);
        }
        self.world.set_movement(Movement::pivot(
            &entity,
            movement_direction,
            entity.direction,
            intermediate,
            Animation::TurnIn,
            1,
        ))?;
        self.apply_pivot_forces_1(id, movement_direction, entity.direction, intermediate)?;
        let mut footing_moved = true;
        if id == self.world.player_id() {
            footing_moved = self.apply_force_at(
                entity.pos + Direction::Down,
                movement_direction,
                0,
                1,
                Some(id),
            )?;
        }
        self.apply_pivot_forces_2(id, movement_direction, entity.direction, intermediate)?;
        self.world.clear_movement(id);
        let hat = self.get_hat(id)?;
        if let Some(hat_id) = hat {
            let _ = self.try_pivot_turn(hat_id, clockwise, Some(movement_direction))?;
        }
        self.passive_force_sweep()?;
        let current = self
            .world
            .entity(id)
            .ok_or_else(|| format!("pivot target {id} disappeared"))?
            .clone();
        self.world.set_movement(Movement::pivot(
            &current,
            movement_direction,
            current.direction,
            intermediate,
            Animation::TurnIn,
            1,
        ))?;
        self.world
            .entity_mut(id)
            .expect("pivot target exists")
            .turn_direction = final_direction;
        let collision = self.world.collides(id, false)?;
        if !footing_moved || collision {
            self.world.restore()?;
            if entity.entity_type == EntityType::Sausage {
                return self.try_push(id, movement_direction, 1, true);
            }
            return Ok(false);
        }
        self.world.discard_backup()?;
        Ok(true)
    }

    fn automatic_turn(&mut self, id: i32, mut turn_speed: i32) -> Result<bool, String> {
        if self.world.movement(id).is_some() {
            return Ok(false);
        }
        let entity = self
            .world
            .entity(id)
            .ok_or_else(|| format!("automatic turn target {id} disappeared"))?
            .clone();
        if !entity.direction.is_diagonal() || !entity.turn_direction.is_cardinal() {
            return Ok(false);
        }
        let final_direction = entity.turn_direction;
        let force_direction =
            Direction::continue_rotation(final_direction, entity.direction).inverse();

        self.world.backup();
        self.world.set_movement(Movement::rotation(
            &entity,
            entity.direction,
            final_direction,
            Animation::TurnOut,
            1,
        ))?;
        let fixed_floor = if id == self.world.player_id() {
            let floor = self.floor_at(entity.pos, false)?;
            if let Some(floor_id) = floor.filter(|floor_id| {
                self.world
                    .entity(*floor_id)
                    .is_some_and(|floor| !floor.entity_type.is_static())
            }) {
                self.world.clear_movement(floor_id);
                let floor = self
                    .world
                    .entity(floor_id)
                    .expect("automatic-turn floor exists")
                    .clone();
                self.world
                    .set_movement(Movement::fixed(&floor, turn_speed))?;
                Some(floor_id)
            } else {
                None
            }
        } else {
            None
        };
        let forced = if self.is_extended(id)? {
            self.apply_force_at(
                entity.pos + final_direction,
                force_direction,
                1,
                1,
                Some(id),
            )?
        } else {
            false
        };
        if forced {
            turn_speed = 1;
        }
        self.world.clear_movement(id);
        if let Some(floor_id) = fixed_floor
            && self
                .world
                .movement(floor_id)
                .is_some_and(|movement| movement.kind != MovementKind::None)
        {
            self.world
                .movement_mut(floor_id)
                .expect("moving automatic-turn floor exists")
                .set_speed(turn_speed);
        }
        let current = self
            .world
            .entity(id)
            .ok_or_else(|| format!("automatic turn target {id} disappeared"))?
            .clone();
        self.world.set_movement(Movement::rotation(
            &current,
            current.direction,
            final_direction,
            Animation::TurnOut,
            turn_speed,
        ))?;
        self.world
            .entity_mut(id)
            .expect("automatic turn target exists")
            .turn_direction = Direction::None;
        if self.world.collides(id, false)? {
            self.world.restore()?;
            if id == self.world.player_id() && self.automatic_pivot_turn(id, None)? {
                return Ok(true);
            }
            return self.rotate_back(id);
        }
        self.world.discard_backup()?;
        if let Some(hat_id) = self.get_hat(id)?
            && self
                .world
                .entity(hat_id)
                .is_some_and(|hat| hat.direction.is_diagonal())
        {
            let _ = self.automatic_turn(hat_id, turn_speed)?;
        }
        self.passive_force_sweep()?;
        Ok(true)
    }

    fn automatic_pivot_turn(
        &mut self,
        id: i32,
        requested_push: Option<Direction>,
    ) -> Result<bool, String> {
        if self.world.movement(id).is_some() {
            return Ok(false);
        }
        let entity = self
            .world
            .entity(id)
            .ok_or_else(|| format!("automatic pivot target {id} disappeared"))?
            .clone();
        let Some(floor_id) = self.floor_at(
            self.world
                .entity(self.world.player_id())
                .ok_or_else(|| "player disappeared during automatic pivot".to_owned())?
                .pos,
            false,
        )?
        else {
            return Ok(false);
        };
        if self
            .world
            .entity(floor_id)
            .is_none_or(|floor| floor.entity_type != EntityType::Island)
        {
            return Ok(false);
        }
        let final_direction = entity.turn_direction;
        if !entity.direction.is_diagonal() || !final_direction.is_cardinal() {
            return Ok(false);
        }
        let movement_direction = requested_push
            .unwrap_or_else(|| Direction::continue_rotation(final_direction, entity.direction));

        self.world.backup();
        if id == self.world.player_id()
            && self
                .world
                .movement(floor_id)
                .is_some_and(|movement| movement.kind == MovementKind::None)
        {
            self.world.clear_movement(floor_id);
        }
        self.world.set_movement(Movement::pivot(
            &entity,
            movement_direction,
            entity.direction,
            final_direction,
            Animation::TurnOut,
            1,
        ))?;
        self.apply_pivot_forces_1(id, movement_direction, entity.direction, final_direction)?;
        let mut footing_moved = true;
        if id == self.world.player_id() {
            footing_moved = self.apply_force_at(
                entity.pos + Direction::Down,
                movement_direction,
                0,
                1,
                Some(id),
            )?;
        }
        self.apply_pivot_forces_2(id, movement_direction, entity.direction, final_direction)?;
        self.world.clear_movement(id);
        self.passive_force_sweep()?;
        let current = self
            .world
            .entity(id)
            .ok_or_else(|| format!("automatic pivot target {id} disappeared"))?
            .clone();
        self.world.set_movement(Movement::pivot(
            &current,
            movement_direction,
            current.direction,
            final_direction,
            Animation::TurnOut,
            1,
        ))?;
        if !footing_moved || self.world.collides(id, false)? {
            self.world.restore()?;
            return Ok(false);
        }
        self.world.discard_backup()?;
        self.world
            .entity_mut(id)
            .expect("automatic pivot target exists")
            .turn_direction = Direction::None;
        Ok(true)
    }

    fn apply_pivot_forces_1(
        &mut self,
        id: i32,
        movement_direction: Direction,
        from: Direction,
        to: Direction,
    ) -> Result<(), String> {
        let pos = self
            .world
            .entity(id)
            .ok_or_else(|| format!("pivot force target {id} disappeared"))?
            .pos;
        if from.is_orthogonal() {
            if from != movement_direction
                && from != movement_direction.inverse()
                && Direction::rotation_between(from, movement_direction) == Some(to)
            {
                let _ = self.apply_force_at(pos + to, from, 1, 2, Some(id))?;
            }
        } else if to != movement_direction
            && to != movement_direction.inverse()
            && Direction::continue_rotation(to, from) != movement_direction
        {
            let _ = self.apply_force_at(pos + to, to, 1, 2, Some(id))?;
        }
        Ok(())
    }

    fn apply_pivot_forces_2(
        &mut self,
        id: i32,
        movement_direction: Direction,
        from: Direction,
        to: Direction,
    ) -> Result<(), String> {
        let pos = self
            .world
            .entity(id)
            .ok_or_else(|| format!("pivot force target {id} disappeared"))?
            .pos;
        if from.is_orthogonal() {
            if from == movement_direction {
                let _ = self.apply_force_at(
                    pos + movement_direction + from,
                    movement_direction,
                    1,
                    1,
                    Some(id),
                )?;
                let _ = self.apply_force_at(
                    pos + movement_direction + to,
                    movement_direction,
                    1,
                    1,
                    Some(id),
                )?;
            } else if from == movement_direction.inverse()
                || Direction::rotation_between(from, movement_direction) == Some(to)
            {
                let _ = self.apply_force_at(
                    pos + movement_direction,
                    movement_direction,
                    1,
                    1,
                    Some(id),
                )?;
                let _ = self.apply_force_at(
                    pos + movement_direction + to,
                    movement_direction,
                    1,
                    1,
                    Some(id),
                )?;
            } else {
                let _ = self.apply_force_at(
                    pos + movement_direction,
                    movement_direction,
                    1,
                    1,
                    Some(id),
                )?;
            }
        } else if to == movement_direction {
            let _ = self.apply_force_at(
                pos + movement_direction + from,
                movement_direction,
                1,
                1,
                Some(id),
            )?;
            let _ = self.apply_force_at(
                pos + movement_direction + to,
                movement_direction,
                1,
                1,
                Some(id),
            )?;
            let side = Direction::continue_rotation(to, from).inverse();
            let _ = self.apply_force_at(pos + movement_direction, side, 1, 1, Some(id))?;
        } else if to == movement_direction.inverse() {
            let side = Direction::continue_rotation(to, from).inverse();
            let _ =
                self.apply_force_at(pos + movement_direction, movement_direction, 1, 1, Some(id))?;
            let _ =
                self.apply_force_at(pos + movement_direction.inverse(), side, 1, 2, Some(id))?;
        } else if Direction::continue_rotation(to, from) == movement_direction {
            let _ =
                self.apply_force_at(pos + movement_direction, movement_direction, 1, 1, Some(id))?;
        } else {
            let _ =
                self.apply_force_at(pos + movement_direction, movement_direction, 1, 1, Some(id))?;
            let _ = self.apply_force_at(
                pos + movement_direction + to,
                movement_direction,
                1,
                1,
                Some(id),
            )?;
        }
        Ok(())
    }

    fn rotate_back(&mut self, id: i32) -> Result<bool, String> {
        let entity = self
            .world
            .entity(id)
            .ok_or_else(|| format!("rotate-back target {id} disappeared"))?
            .clone();
        if !entity.direction.is_diagonal() {
            return Ok(false);
        }
        let target = Direction::continue_rotation(entity.turn_direction, entity.direction);
        if !target.is_cardinal() {
            return Ok(false);
        }
        self.world.backup();
        self.world.set_movement(Movement::rotation(
            &entity,
            entity.direction,
            target,
            Animation::TurnBackout,
            2,
        ))?;
        self.world
            .entity_mut(id)
            .expect("rotate-back target exists")
            .turn_direction = Direction::None;
        if self.world.collides(id, false)? {
            self.world.restore()?;
            return Ok(false);
        }
        self.world.discard_backup()?;
        Ok(true)
    }

    fn try_push(
        &mut self,
        id: i32,
        direction: Direction,
        speed: i32,
        force_zero_torsion: bool,
    ) -> Result<bool, String> {
        let entity = self
            .world
            .entity(id)
            .ok_or_else(|| format!("push target {id} disappeared"))?
            .clone();
        if entity.entity_type.is_static()
            || entity.entity_type == EntityType::Barrier
            || self.world.movement(id).is_some()
            || (entity.entity_type == EntityType::Island
                && (entity.data == self.push_target_level || self.overworld))
        {
            return Ok(false);
        }

        self.world.backup();
        let torsion = if !force_zero_torsion
            && entity.entity_type == EntityType::Sausage
            && !entity.direction.parallel_to(direction)
            && !direction.is_vertical()
        {
            -666
        } else {
            0
        };
        self.set_movement_with_attachment(Movement::translation(
            &entity,
            direction,
            torsion,
            speed,
            Animation::Idle,
            true,
            0,
        ))?;
        if !direction.is_vertical() {
            self.passive_force_sweep_with_torque(false)?;
        }
        self.apply_force_from(id, direction, 1, speed)?;
        let resolved_torsion = self
            .world
            .movement(id)
            .ok_or_else(|| format!("temporary movement for {id} disappeared"))?
            .torsion;
        self.clear_movement_with_attachment(id);
        let current = self
            .world
            .entity(id)
            .ok_or_else(|| format!("push target {id} disappeared"))?
            .clone();
        self.set_movement_with_attachment(Movement::translation(
            &current,
            direction,
            torsion,
            speed,
            Animation::Idle,
            true,
            0,
        ))?;
        if current.entity_type == EntityType::Fork
            && current.stuck_to < 0
            && current.direction == direction
        {
            let target_pos = current.pos + direction;
            if let Some(sausage_id) =
                self.world
                    .entities_at(target_pos, true)?
                    .into_iter()
                    .find(|target| {
                        self.world.entity(*target).is_some_and(|entity| {
                            entity.entity_type == EntityType::Sausage
                                && self.world.movement(*target).is_none()
                        })
                    })
            {
                self.world
                    .entity_mut(current.id)
                    .expect("moving fork exists")
                    .stuck_to = sausage_id;
                self.world
                    .entity_mut(sausage_id)
                    .expect("skewered sausage exists")
                    .stuck_to = current.id;
            }
        }
        let collision = self.world.collides(id, false)?;
        if collision {
            self.world.restore()?;
            return Ok(false);
        }
        if !direction.is_vertical()
            && (current.entity_type == EntityType::Island
                || self
                    .world
                    .movement(id)
                    .is_some_and(|movement| movement.torsion == -666))
        {
            self.passive_force_sweep()?;
        }
        let final_torsion = self
            .world
            .movement(id)
            .ok_or_else(|| format!("final movement for {id} disappeared"))?
            .torsion;
        if resolved_torsion != final_torsion {
            self.world.restore()?;
            return self.try_push(id, direction, speed, true);
        }
        self.world.discard_backup()?;
        Ok(true)
    }

    fn apply_force_from(
        &mut self,
        id: i32,
        direction: Direction,
        _torsion: i32,
        speed: i32,
    ) -> Result<bool, String> {
        let border = self.world.border(id, direction)?;
        let mut targets = Vec::new();
        let mut seen = HashSet::new();
        for pos in border {
            for target in self.world.entities_at(pos, true)? {
                if target != id && seen.insert(target) {
                    targets.push(target);
                }
            }
        }
        let mut found = false;
        let mut all_moved = true;
        for target in targets {
            let entity = self
                .world
                .entity(target)
                .ok_or_else(|| format!("force target {target} disappeared"))?;
            if self.world.movement(target).is_some()
                || (entity.entity_type == EntityType::Fork && entity.stuck_to >= 0)
                || (entity.entity_type == EntityType::Player && !direction.is_vertical())
            {
                continue;
            }
            found = true;
            if !self.try_push(target, direction, speed, false)? {
                all_moved = false;
            }
        }
        Ok(found && all_moved)
    }

    fn apply_force_at(
        &mut self,
        pos: Coord,
        direction: Direction,
        _torsion: i32,
        speed: i32,
        from: Option<i32>,
    ) -> Result<bool, String> {
        for target in self.world.entities_at(pos, true)? {
            if Some(target) == from || self.world.movement(target).is_some() {
                continue;
            }
            let entity = self
                .world
                .entity(target)
                .ok_or_else(|| format!("force target {target} disappeared"))?;
            if entity.entity_type == EntityType::Fork && entity.stuck_to >= 0 {
                continue;
            }
            return self.try_push(target, direction, speed, false);
        }
        Ok(false)
    }

    fn floor_at(&self, pos: Coord, instant: bool) -> Result<Option<i32>, String> {
        let fork_id = self.world.fork_id();
        let mut candidates = self
            .world
            .entities_at(pos + Direction::Down, instant)?
            .into_iter()
            .filter(|id| {
                fork_id != Some(*id) || self.world.entity(*id).is_some_and(|fork| fork.stuck_to < 0)
            })
            .collect::<Vec<_>>();
        candidates.sort_by_key(|id| {
            self.world
                .entity(*id)
                .map_or(usize::MAX, |entity| match entity.entity_type {
                    EntityType::Sausage => 0,
                    EntityType::Fork => 1,
                    EntityType::Player => 2,
                    EntityType::Island => 3,
                    EntityType::Barrier => 4,
                    _ => 5,
                })
        });
        Ok(candidates.into_iter().next())
    }

    fn under(&self, id: i32) -> Result<Vec<i32>, String> {
        let footprint = self.world.source_footprint(id)?;
        let mut result = Vec::new();
        for pos in footprint {
            if let Some(floor) = self.floor_at(pos, false)?
                && !result.contains(&floor)
            {
                result.push(floor);
            }
        }
        Ok(result)
    }

    fn process_gravity(&mut self) -> Result<bool, String> {
        self.try_reattach_fork()?;
        let candidates = self
            .world
            .dynamic_entity_ids()
            .filter(|id| {
                self.world.entity(*id).is_some_and(|entity| {
                    matches!(
                        entity.entity_type,
                        EntityType::Player
                            | EntityType::Sausage
                            | EntityType::Fork
                            | EntityType::Island
                    )
                })
            })
            .filter(|id| self.can_fall_liberal(*id, true))
            .collect::<Vec<_>>();
        let mut fallers = Vec::new();
        for id in candidates {
            if self.world.movement(id).is_some() {
                continue;
            }
            let entity = self
                .world
                .entity(id)
                .ok_or_else(|| format!("gravity target {id} disappeared"))?
                .clone();
            let attached_id = (entity.stuck_to >= 0).then_some(entity.stuck_to);
            self.set_movement_with_attachment(Movement::translation(
                &entity,
                Direction::Down,
                0,
                1,
                if entity.entity_type == EntityType::Player {
                    Animation::Fall
                } else {
                    Animation::Idle
                },
                true,
                0,
            ))
            .map_err(|error| format!("gravity could not start entity {id}: {error}"))?;
            fallers.push(id);
            if let Some(attached_id) =
                attached_id.filter(|attached| self.world.movement(*attached).is_some())
                && !fallers.contains(&attached_id)
            {
                fallers.push(attached_id);
            }
        }
        let mut changed = true;
        while changed {
            changed = false;
            let current = fallers.clone();
            for id in current.into_iter().rev() {
                if self.world.movement(id).is_some() && self.world.collides(id, false)? {
                    let attached_id = self
                        .world
                        .entity(id)
                        .and_then(|entity| (entity.stuck_to >= 0).then_some(entity.stuck_to));
                    self.world.clear_movement(id);
                    fallers.retain(|candidate| *candidate != id);
                    if let Some(attached_id) = attached_id {
                        self.world.clear_movement(attached_id);
                        fallers.retain(|candidate| *candidate != attached_id);
                    }
                    changed = true;
                }
            }
        }
        self.try_detach_fork()?;
        Ok(!fallers.is_empty())
    }

    fn try_detach_fork(&mut self) -> Result<(), String> {
        let player_id = self.world.player_id();
        let player = self
            .world
            .entity(player_id)
            .ok_or_else(|| "player disappeared while detaching fork".to_owned())?
            .clone();
        let has_footing = if let Some(player) = self.world.entity(player_id) {
            self.floor_at(player.pos, false)?.is_some()
        } else {
            false
        };
        if !self.world.player_has_fork()
            || player.pos.z < -2
            || !player.data.is_empty()
            || has_footing
            || self
                .world
                .movement(player_id)
                .is_some_and(|movement| movement.direction != Direction::Down)
            || self.world.dynamic_entity_ids().any(|id| {
                self.world
                    .movement(id)
                    .is_some_and(|movement| !movement.direction.is_vertical())
            })
        {
            return Ok(());
        }

        let attached_id = (player.stuck_to >= 0).then_some(player.stuck_to);
        let mut fork = player.clone();
        fork.id = -1;
        fork.pos = player.pos + player.direction;
        fork.entity_type = EntityType::Fork;
        fork.data.clear();
        fork.stuck_to = attached_id.unwrap_or(-1);
        fork.rotation = 0;
        fork.cook_data = 0;
        fork.turn_direction = Direction::None;
        fork.tile_number = 0;
        fork.tile_set = 0;
        fork.pivot = 0;
        let fork_id = self.world.insert_fork(fork)?;
        let player = self
            .world
            .entity_mut(player_id)
            .expect("player exists while detaching fork");
        player.data.clear();
        // `cookdata` is only an occupancy-cache invalidation marker in the
        // Unity implementation. The serialized authoritative state keeps it
        // at zero once the detached fork entity exists.
        player.cook_data = 0;
        player.stuck_to = -1;
        if let Some(attached_id) = attached_id {
            self.world
                .entity_mut(attached_id)
                .ok_or_else(|| format!("fork attachment {attached_id} disappeared"))?
                .stuck_to = fork_id;
        }
        Ok(())
    }

    fn try_reattach_fork(&mut self) -> Result<(), String> {
        let Some(fork_id) = self.world.fork_id() else {
            return Ok(());
        };
        let player_id = self.world.player_id();
        let player = self
            .world
            .entity(player_id)
            .ok_or_else(|| "player disappeared while reattaching fork".to_owned())?
            .clone();
        let fork = self
            .world
            .entity(fork_id)
            .ok_or_else(|| format!("detached fork {fork_id} disappeared"))?
            .clone();
        if self.world.movement(player_id).is_some()
            || self.world.movement(fork_id).is_some()
            || (player.data.is_empty() && self.floor_at(player.pos, false)?.is_none())
            || player.pos + player.direction != fork.pos
            || player.direction != fork.direction
        {
            return Ok(());
        }
        let attached_id = (fork.stuck_to >= 0).then_some(fork.stuck_to);
        if let Some(attached_id) = attached_id {
            self.world
                .entity_mut(attached_id)
                .ok_or_else(|| format!("fork attachment {attached_id} disappeared"))?
                .stuck_to = player_id;
        }
        let _fork = self.world.remove_fork()?;
        let player = self
            .world
            .entity_mut(player_id)
            .expect("player exists while reattaching fork");
        player.cook_data = 0;
        player.stuck_to = attached_id.unwrap_or(-1);
        Ok(())
    }

    fn can_fall_liberal(&self, id: i32, recurse: bool) -> bool {
        let Some(entity) = self.world.entity(id) else {
            return false;
        };
        if self.world.movement(id).is_some() || entity.pos.z < -10 {
            return false;
        }
        if entity.entity_type == EntityType::Island {
            return !self.overworld && entity.data != self.push_target_level;
        }
        if entity.entity_type == EntityType::Player && !entity.data.is_empty() {
            return false;
        }
        if recurse && entity.stuck_to >= 0 {
            return self.can_fall_liberal(entity.stuck_to, false);
        }
        true
    }

    fn cook(&mut self, id: i32) -> Result<(), String> {
        if self.overworld {
            return Ok(());
        }
        let entity = self
            .world
            .entity(id)
            .ok_or_else(|| format!("cook target {id} disappeared"))?
            .clone();
        if entity.entity_type != EntityType::Sausage || entity.pos.z <= -2 {
            return Ok(());
        }
        let first_pos = entity.pos;
        let second_pos = entity.pos + entity.direction;
        let (first_grill, first_key) = self.bbq_at(first_pos + Direction::Down)?;
        let (second_grill, second_key) = self.bbq_at(second_pos + Direction::Down)?;
        if first_grill == Direction::None && second_grill == Direction::None {
            let target = self.world.entity_mut(id).expect("cook target exists");
            target.data = match target.data.chars().next() {
                Some('L') => "L;;".to_owned(),
                Some('S') => "S;;".to_owned(),
                _ => "M;;".to_owned(),
            };
            return Ok(());
        }

        let mut faces = [
            entity.cook_data % 4,
            entity.cook_data / 4 % 4,
            entity.cook_data / 16 % 4,
            entity.cook_data / 64 % 4,
        ];
        let stored = entity.data.split(';').collect::<Vec<_>>();
        let first_changed =
            first_grill != Direction::None && stored.get(1).copied() != Some(first_key.as_str());
        let second_changed =
            second_grill != Direction::None && stored.get(2).copied() != Some(second_key.as_str());
        if first_changed {
            let face = if entity.rotation == 0 { 3 } else { 2 };
            cook_face(&mut faces[face], first_grill, entity.direction);
        }
        if second_changed {
            let face = if entity.rotation == 0 { 0 } else { 1 };
            cook_face(&mut faces[face], second_grill, entity.direction);
        }
        let target = self.world.entity_mut(id).expect("cook target exists");
        target.cook_data = faces[0] + 4 * faces[1] + 16 * faces[2] + 64 * faces[3];
        let status = if faces.into_iter().any(|face| face > 2) {
            'B'
        } else {
            entity.data.chars().next().unwrap_or('M')
        };
        target.data = format!("{status};{first_key};{second_key}");
        Ok(())
    }

    fn bbq_at(&self, pos: Coord) -> Result<(Direction, String), String> {
        for id in self.world.entities_at(pos, false)? {
            let entity = self
                .world
                .entity(id)
                .ok_or_else(|| format!("BBQ entity {id} disappeared"))?;
            if entity.entity_type == EntityType::Bbq {
                return Ok((entity.direction, id.to_string()));
            }
            if entity.entity_type == EntityType::Island {
                let (value, local) = self.world.island_mask_value(id, pos)?;
                let direction = match value {
                    2 => Direction::East,
                    20 => Direction::North,
                    _ => Direction::None,
                };
                if direction != Direction::None {
                    return Ok((
                        direction,
                        format!("{}.{}.{}.{}", entity.data, local.x, local.y, local.z),
                    ));
                }
            }
        }
        Ok((Direction::None, String::new()))
    }

    fn passive_force_sweep(&mut self) -> Result<(), String> {
        self.passive_force_sweep_with_torque(true)
    }

    fn passive_force_sweep_with_torque(&mut self, apply_torque: bool) -> Result<(), String> {
        if !self.world.moving() {
            return Ok(());
        }
        let initial_horizontal_movers = self
            .world
            .dynamic_entity_ids()
            .filter(|id| {
                self.world.movement(*id).is_some_and(|movement| {
                    movement.kind == MovementKind::Translation && !movement.direction.is_vertical()
                })
            })
            .collect::<Vec<_>>();
        if initial_horizontal_movers.is_empty() {
            return Ok(());
        }
        let sweep_direction = initial_horizontal_movers
            .iter()
            .find_map(|id| {
                self.world
                    .movement(*id)
                    .map(|movement| movement.effective_direction())
            })
            .unwrap_or(Direction::None);
        self.calculate_torsions(apply_torque)?;
        let mut changed = true;
        while changed {
            changed = false;
            let horizontal_movers = self
                .world
                .dynamic_entity_ids()
                .filter(|id| {
                    self.world.movement(*id).is_some_and(|movement| {
                        movement.kind == MovementKind::Translation
                            && !movement.direction.is_vertical()
                    })
                })
                .collect::<Vec<_>>();
            let candidates = self
                .world
                .dynamic_entity_ids()
                .filter(|id| *id != self.world.player_id())
                .filter(|id| {
                    self.world.entity(*id).is_some_and(|entity| {
                        matches!(
                            entity.entity_type,
                            EntityType::Sausage | EntityType::Fork | EntityType::Island
                        ) && (entity.stuck_to < 0
                            || self
                                .is_extended(entity.stuck_to)
                                .is_ok_and(|extended| !extended))
                            && (apply_torque
                                || entity.entity_type != EntityType::Sausage
                                || entity.direction.parallel_to(sweep_direction))
                    })
                })
                .filter(|id| {
                    self.may_rest_on_any(*id, &horizontal_movers)
                        .unwrap_or(true)
                })
                .collect::<Vec<_>>();
            for id in candidates {
                if self.world.movement(id).is_none() && self.apply_passive_force(id)? {
                    self.calculate_torsions(apply_torque)?;
                    changed = true;
                }
            }
        }
        Ok(())
    }

    fn may_rest_on_any(&self, id: i32, movers: &[i32]) -> Result<bool, String> {
        let (subject_min, subject_max) = self.world.footprint_bounds(id)?;
        let lower_min = subject_min + Direction::Down;
        let lower_max = subject_max + Direction::Down;
        for mover in movers {
            let (mut mover_min, mut mover_max) = self.world.footprint_bounds(*mover)?;
            if let Some(movement) = self.world.movement(*mover) {
                let target_min = mover_min + movement.direction;
                let target_max = mover_max + movement.direction;
                mover_min = Coord::new(
                    mover_min.x.min(target_min.x),
                    mover_min.y.min(target_min.y),
                    mover_min.z.min(target_min.z),
                );
                mover_max = Coord::new(
                    mover_max.x.max(target_max.x),
                    mover_max.y.max(target_max.y),
                    mover_max.z.max(target_max.z),
                );
            }
            if boxes_overlap(lower_min, lower_max, mover_min, mover_max) {
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn apply_passive_force(&mut self, id: i32) -> Result<bool, String> {
        let supports = self.footprint_entities(id)?;
        if supports.is_empty() {
            return Ok(false);
        }
        let moving_supports = supports
            .iter()
            .copied()
            .filter(|support| self.world.movement(*support).is_some())
            .collect::<Vec<_>>();
        if moving_supports.is_empty()
            || !moving_supports.iter().any(|support| {
                self.world.movement(*support).is_some_and(|movement| {
                    movement.kind == MovementKind::Translation && !movement.direction.is_vertical()
                })
            })
        {
            return Ok(false);
        }
        let mixed_footing = moving_supports.len() != supports.len();
        if !mixed_footing && !self.consistent_footprint(&supports) {
            return Ok(false);
        }
        if mixed_footing {
            self.world.backup();
        }

        let mut force = self.extract_passive_force(moving_supports[0])?;
        for support in moving_supports.into_iter().skip(1) {
            force = merge_forces(force, self.extract_passive_force(support)?);
        }
        if force.direction == Direction::None {
            if mixed_footing {
                self.world.discard_backup()?;
            }
            return Ok(false);
        }
        let entity = self
            .world
            .entity(id)
            .ok_or_else(|| format!("passive target {id} disappeared"))?
            .clone();
        let mut moved = false;
        if (self.is_extended(id)? && force.direction.normal_to(entity.direction))
            || force.torsion == 0
        {
            moved = self.try_push(id, force.direction, force.speed, false)?;
        } else if force.torsion != -1 {
            let fast = self.try_push(id, force.direction, force.speed + 1, true)?;
            if fast && self.consistent_footprint(&supports) {
                moved = true;
            } else {
                if fast && mixed_footing {
                    self.world.restore()?;
                    self.world.backup();
                }
                moved = self.try_push(id, force.direction, force.speed, true)?;
            }
        }

        if moved {
            let moving_islands = self
                .world
                .dynamic_entity_ids()
                .filter(|target| {
                    self.world
                        .entity(*target)
                        .is_some_and(|entity| entity.entity_type == EntityType::Island)
                        && self
                            .world
                            .movement(*target)
                            .is_some_and(|movement| movement.kind != MovementKind::None)
                })
                .collect::<Vec<_>>();
            for island_id in moving_islands {
                let footing = self.footprint_entities(island_id)?;
                if !self.consistent_footprint(&footing) {
                    if mixed_footing {
                        self.world.restore()?;
                    }
                    return Ok(false);
                }
            }
        }

        if mixed_footing {
            if moved && self.consistent_footprint(&supports) {
                self.world.discard_backup()?;
            } else {
                self.world.restore()?;
                return Ok(false);
            }
        }
        Ok(moved)
    }

    fn footprint_entities(&self, id: i32) -> Result<Vec<i32>, String> {
        let mut result = Vec::new();
        for pos in self.world.lower_footprint(id)? {
            for support in self.world.entities_at(pos, true)? {
                if support != id
                    && self
                        .world
                        .movement(support)
                        .is_none_or(|movement| movement.direction != Direction::Down)
                    && !result.contains(&support)
                {
                    result.push(support);
                }
            }
        }
        Ok(result)
    }

    fn consistent_footprint(&self, supports: &[i32]) -> bool {
        !supports.is_empty()
            && supports.iter().all(|support| {
                self.world.movement(*support).is_some_and(|movement| {
                    movement.kind == MovementKind::Translation
                        && movement.direction != Direction::None
                })
            })
    }

    fn extract_passive_force(&mut self, id: i32) -> Result<PassiveForce, String> {
        let Some(movement) = self.world.movement(id) else {
            return Ok(PassiveForce {
                direction: Direction::None,
                speed: 0,
                torsion: 0,
            });
        };
        if matches!(movement.kind, MovementKind::Rotation | MovementKind::None) {
            return Ok(PassiveForce {
                direction: Direction::None,
                speed: 0,
                torsion: 0,
            });
        }
        if movement.torsion == -666 {
            self.calculate_torsion(id)?;
        }
        let movement = self
            .world
            .movement(id)
            .ok_or_else(|| format!("movement for {id} disappeared"))?;
        Ok(PassiveForce {
            direction: movement.direction,
            speed: movement.speed,
            torsion: movement.torsion,
        })
    }

    fn calculate_torsions(&mut self, apply_torque: bool) -> Result<(), String> {
        let ids = self
            .world
            .dynamic_entity_ids()
            .filter(|id| {
                self.world
                    .movement(*id)
                    .is_some_and(|movement| movement.torsion == -666)
            })
            .collect::<Vec<_>>();
        for id in ids {
            self.calculate_torsion(id)?;
            let movement = self
                .world
                .movement(id)
                .ok_or_else(|| format!("movement for {id} disappeared"))?;
            if apply_torque && movement.torsion != 0 {
                let direction = movement.direction;
                self.try_rotate_entity(id, direction)?;
            }
        }
        Ok(())
    }

    fn calculate_torsion(&mut self, id: i32) -> Result<(), String> {
        let movement = self
            .world
            .movement(id)
            .ok_or_else(|| format!("movement for {id} disappeared"))?;
        let entity = self
            .world
            .entity(id)
            .ok_or_else(|| format!("torsion target {id} disappeared"))?
            .clone();
        let torsion = if entity.entity_type == EntityType::Island
            || entity.direction.parallel_to(movement.direction)
            || entity.entity_type != EntityType::Sausage
            || movement.direction.is_vertical()
        {
            0
        } else {
            let supports = self.footprint_entities(id)?;
            if supports.is_empty() {
                0
            } else {
                let mut force = self.extract_passive_force(supports[0])?;
                for support in supports.iter().copied().skip(1) {
                    force = merge_forces(force, self.extract_passive_force(support)?);
                }
                if force.direction.is_vertical()
                    || (force.direction == Direction::None && force.speed > 0)
                {
                    0
                } else if force.torsion != 0 {
                    let mut lowered = self.world.occupancy(id)?;
                    for occupancy in &mut lowered {
                        occupancy.pos = occupancy.pos + Direction::Down;
                    }
                    let overlapping = supports
                        .iter()
                        .filter_map(|support| {
                            let occupancy = self.world.occupancy(*support).ok()?;
                            occupancy
                                .iter()
                                .any(|left| lowered.iter().any(|right| left.overlaps(*right)))
                                .then_some(*support)
                        })
                        .collect::<Vec<_>>();
                    if overlapping.is_empty() {
                        0
                    } else {
                        let first = self
                            .world
                            .movement(overlapping[0])
                            .expect("overlapping support is moving");
                        let mut support_speed = first.speed - first.torsion;
                        if overlapping.iter().skip(1).any(|support| {
                            self.world.movement(*support).is_none_or(|movement| {
                                movement.speed - movement.torsion != support_speed
                            })
                        }) {
                            support_speed = 0;
                        }
                        if movement.speed > support_speed + 1 {
                            1
                        } else if movement.speed == support_speed + 1 {
                            -1
                        } else if movement.speed == support_speed - 1 {
                            1
                        } else {
                            0
                        }
                    }
                } else if movement.speed == force.speed {
                    0
                } else if movement.speed < force.speed {
                    -1
                } else {
                    1
                }
            }
        };
        self.world
            .movement_mut(id)
            .ok_or_else(|| format!("movement for {id} disappeared"))?
            .torsion = torsion;
        Ok(())
    }

    fn get_hat(&mut self, id: i32) -> Result<Option<i32>, String> {
        let entity = self
            .world
            .entity(id)
            .ok_or_else(|| format!("hat bearer {id} disappeared"))?
            .clone();
        let Some(hat_id) = self
            .world
            .entities_at(entity.pos + Direction::Up, false)?
            .into_iter()
            .find(|candidate| {
                self.world.entity(*candidate).is_some_and(|hat| {
                    matches!(hat.entity_type, EntityType::Sausage | EntityType::Fork)
                })
            })
        else {
            return Ok(None);
        };
        let should_pivot = self.world.entity(hat_id).is_some_and(|hat| {
            hat.entity_type == EntityType::Sausage && hat.pos != entity.pos + Direction::Up
        });
        if should_pivot {
            self.world
                .entity_mut(hat_id)
                .expect("hat exists")
                .pivot_in_place();
        }
        let hat = self
            .world
            .entity(hat_id)
            .ok_or_else(|| format!("hat {hat_id} disappeared"))?;
        if self.under(hat_id)?.len() == 1 || hat.direction.is_diagonal() {
            Ok(Some(hat_id))
        } else {
            Ok(None)
        }
    }

    fn is_extended(&self, id: i32) -> Result<bool, String> {
        let entity = self
            .world
            .entity(id)
            .ok_or_else(|| format!("unknown entity id {id}"))?;
        Ok(entity.entity_type.is_extended()
            || (entity.entity_type == EntityType::Player && self.world.player_has_fork()))
    }

    fn try_rotate_entity(&mut self, id: i32, force_direction: Direction) -> Result<(), String> {
        let entity = self
            .world
            .entity(id)
            .ok_or_else(|| format!("rotation target {id} disappeared"))?
            .clone();
        if !entity.direction.is_valid() || !force_direction.parallel_to(entity.direction) {
            self.world
                .entity_mut(id)
                .expect("rotation target exists")
                .rotation = 1 - entity.rotation;
            if self.world.fork_id() == Some(entity.stuck_to)
                && self
                    .world
                    .entity(entity.stuck_to)
                    .is_some_and(|fork| entity.direction.normal_to(fork.direction))
            {
                let fork = self
                    .world
                    .entity_mut(entity.stuck_to)
                    .expect("attached fork exists");
                fork.direction = fork.direction.inverse();
            }
        }
        Ok(())
    }
}

fn merge_forces(left: PassiveForce, right: PassiveForce) -> PassiveForce {
    if left.direction != right.direction {
        return PassiveForce {
            direction: Direction::None,
            speed: 1,
            torsion: 0,
        };
    }
    if left.direction == Direction::None {
        return PassiveForce {
            direction: Direction::None,
            speed: left.speed.max(right.speed),
            torsion: 0,
        };
    }
    PassiveForce {
        direction: left.direction,
        speed: left.speed.min(right.speed),
        torsion: if left.torsion == right.torsion {
            left.torsion
        } else {
            0
        },
    }
}

fn boxes_overlap(left_min: Coord, left_max: Coord, right_min: Coord, right_max: Coord) -> bool {
    left_min.x <= right_max.x
        && left_max.x >= right_min.x
        && left_min.y <= right_max.y
        && left_max.y >= right_min.y
        && left_min.z <= right_max.z
        && left_max.z >= right_min.z
}

fn cook_face(face: &mut i32, grill: Direction, sausage: Direction) {
    if *face == 0 {
        *face = if grill.parallel_to(sausage) { 2 } else { 1 };
    } else {
        *face = 3;
    }
}

#[cfg(test)]
mod tests {
    use crate::{OracleCampaign, OracleCheckpoint};

    use super::*;

    fn jenga() -> (Campaign, crate::OracleSegment) {
        let root = crate::data_root();
        let campaign =
            Campaign::load_gzip(root.join("campaign").join("merged_binary.gz")).expect("campaign");
        let oracle =
            OracleCampaign::load(root.join("oracle").join("segments.tar.gz")).expect("oracle");
        let segment = oracle
            .segments
            .into_iter()
            .find(|segment| segment.id == "jenga3")
            .expect("jenga3");
        (campaign, segment)
    }

    fn assert_checkpoint(
        game: &Game3d<'_>,
        checkpoint: &OracleCheckpoint,
        puzzle: &str,
        action: usize,
    ) {
        let mut differences = Vec::new();
        for expected in &checkpoint.entities {
            let id = expected.id;
            let actual = game
                .world
                .entity(id)
                .unwrap_or_else(|| panic!("missing entity {id}"));
            let actual = (
                actual.pos,
                actual.direction,
                actual.rotation,
                actual.cook_data,
            );
            let expected = (
                expected.pos,
                expected.direction,
                expected.rotation,
                expected.cook_data,
            );
            if actual != expected {
                differences.push(format!(
                    "entity {id}: actual={actual:?} expected={expected:?}"
                ));
            }
        }
        assert!(
            differences.is_empty(),
            "{puzzle} action {action}\n{}",
            differences.join("\n")
        );
    }

    #[test]
    fn great_tower_complete_replay_uses_the_general_engine() {
        let (campaign, segment) = jenga();
        let mut game = Game3d::from_state(&campaign, &segment.entry).expect("3D game");
        for index in 0..segment.replay.directions.len() {
            let direction = segment.replay.directions[index];
            assert!(
                game.step(direction).unwrap_or_else(|error| {
                    panic!("action {} engine error: {error}", index + 1)
                }),
                "action {} was rejected",
                index + 1
            );
            if let Some(checkpoint) = segment.checkpoints.get(index) {
                assert_checkpoint(&game, checkpoint, &segment.id, index + 1);
            }
        }
        assert!(game.complete(), "Great Tower did not complete at its entry");
    }

    #[test]
    fn exit_tracks_the_sausage_it_is_attached_to() {
        let root = crate::data_root();
        let campaign =
            Campaign::load_gzip(root.join("campaign").join("merged_binary.gz")).expect("campaign");
        let oracle =
            OracleCampaign::load(root.join("oracle").join("segments.tar.gz")).expect("oracle");
        let segment = oracle
            .segments
            .iter()
            .find(|segment| segment.id == "islandshape46d__island1")
            .expect("moving-island puzzle");
        let mut game = Game3d::from_state(&campaign, &segment.entry).expect("3D game");
        for direction in &segment.replay.directions {
            assert!(game.step(*direction).expect("walkthrough input"));
        }
        let player = game.world.entity(game.world.player_id()).expect("player");
        assert!(
            game.can_exit(),
            "won={} held_fork={} player={:?}/{:?} exit={:?}/{:?} exit_up={}",
            game.won(),
            game.world.player_has_fork(),
            player.pos,
            player.direction,
            game.exit_pos,
            game.exit_direction,
            game.exit_up
        );
    }
}
