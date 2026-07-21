//! Exact discrete-time primitives used by the general three-dimensional engine.
//!
//! The game resolves several entities concurrently. A cell is therefore not
//! simply occupied or empty: an entity can be leaving it while another enters
//! it at a compatible speed. These types preserve that timing model without
//! carrying any rendering state.

use std::cmp::Ordering;
use std::fmt;
use std::ops::{Add, Div, Mul, Sub};

use crate::{Coord, Direction, Entity, EntityType};

#[derive(Clone, Copy, Debug)]
pub struct Fraction {
    pub numerator: i64,
    pub denominator: i64,
}

impl Fraction {
    pub const ZERO: Self = Self::new_unchecked(0, 1);
    pub const ONE: Self = Self::new_unchecked(1, 1);

    const fn new_unchecked(numerator: i64, denominator: i64) -> Self {
        Self {
            numerator,
            denominator,
        }
    }

    pub fn new(mut numerator: i64, mut denominator: i64) -> Self {
        assert_ne!(denominator, 0, "fraction denominator cannot be zero");
        if denominator < 0 {
            numerator = -numerator;
            denominator = -denominator;
        }
        if denominator > 10_000 {
            let divisor = gcd(numerator.unsigned_abs(), denominator as u64) as i64;
            numerator /= divisor;
            denominator /= divisor;
        }
        Self {
            numerator,
            denominator,
        }
    }

    pub fn inverse(self) -> Self {
        Self::new(self.denominator, self.numerator)
    }
}

impl From<i32> for Fraction {
    fn from(value: i32) -> Self {
        Self::new(i64::from(value), 1)
    }
}

impl Add for Fraction {
    type Output = Self;

    fn add(self, rhs: Self) -> Self::Output {
        Self::new(
            self.numerator * rhs.denominator + rhs.numerator * self.denominator,
            self.denominator * rhs.denominator,
        )
    }
}

impl Sub for Fraction {
    type Output = Self;

    fn sub(self, rhs: Self) -> Self::Output {
        Self::new(
            self.numerator * rhs.denominator - rhs.numerator * self.denominator,
            self.denominator * rhs.denominator,
        )
    }
}

impl Mul for Fraction {
    type Output = Self;

    fn mul(self, rhs: Self) -> Self::Output {
        Self::new(
            self.numerator * rhs.numerator,
            self.denominator * rhs.denominator,
        )
    }
}

impl Div for Fraction {
    type Output = Self;

    fn div(self, rhs: Self) -> Self::Output {
        Self::new(
            self.numerator * rhs.denominator,
            self.denominator * rhs.numerator,
        )
    }
}

impl PartialEq for Fraction {
    fn eq(&self, other: &Self) -> bool {
        self.numerator * other.denominator == other.numerator * self.denominator
    }
}

impl Eq for Fraction {}

impl PartialOrd for Fraction {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Fraction {
    fn cmp(&self, other: &Self) -> Ordering {
        (self.numerator * other.denominator).cmp(&(other.numerator * self.denominator))
    }
}

impl fmt::Display for Fraction {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.denominator == 1 {
            write!(formatter, "{}", self.numerator)
        } else {
            write!(formatter, "{}/{}", self.numerator, self.denominator)
        }
    }
}

fn gcd(mut left: u64, mut right: u64) -> u64 {
    if left > right {
        std::mem::swap(&mut left, &mut right);
    }
    while right != 0 {
        let remainder = left % right;
        left = right;
        right = remainder;
    }
    left.max(1)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Occupancy {
    pub pos: Coord,
    pub direction: Direction,
    pub entering: bool,
    pub position: Fraction,
    pub speed: i32,
}

impl Occupancy {
    pub const fn new(
        pos: Coord,
        direction: Direction,
        entering: bool,
        position: Fraction,
        speed: i32,
    ) -> Self {
        Self {
            pos,
            direction,
            entering,
            position,
            speed,
        }
    }

    pub fn overlaps(self, other: Self) -> bool {
        !self.compatible_with(other)
    }

    pub fn compatible_with(self, other: Self) -> bool {
        if self.pos != other.pos {
            return true;
        }
        if self.is_static() || other.is_static() {
            return false;
        }
        if self.direction != other.direction || self.entering == other.entering {
            return false;
        }
        let (entering, leaving) = if self.entering {
            (self, other)
        } else {
            (other, self)
        };
        if entering.position > leaving.position {
            return false;
        }
        leaving.time_until_leave() <= entering.time_until_leave()
    }

    pub const fn is_static(self) -> bool {
        self.speed == 0
    }

    pub const fn is_rotating(self) -> bool {
        matches!(self.direction, Direction::None) && self.speed > 0
    }

    fn time_until_leave(self) -> Fraction {
        (Fraction::ONE - self.position) / Fraction::from(self.speed)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum MovementKind {
    None,
    Translation,
    Rotation,
    Pivot,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum Animation {
    Idle,
    WalkForward,
    ForwardPedal,
    Backpedal,
    TurnIn,
    TurnOut,
    TurnBackout,
    StrafeLeft,
    StrafeRight,
    ClimbUpInit,
    ClimbUpLoop,
    ClimbUpEnd1,
    ClimbUpEnd2,
    ClimbDownInit1,
    ClimbDownInit2,
    ClimbDownLoop,
    ClimbDownEnd,
    Fall,
    NoCanDo,
    SurpriseChasm,
    PivotIn,
    PivotOut,
    Fixed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Movement {
    pub target_id: i32,
    pub kind: MovementKind,
    pub direction: Direction,
    pub torsion: i32,
    pub remaining: Fraction,
    pub speed: i32,
    pub from: Direction,
    pub to: Direction,
    pub animation: Animation,
    pub left: bool,
    pub tower_level: i32,
    pub pure_direct_force: bool,
}

impl Movement {
    pub fn translation(
        target: &Entity,
        direction: Direction,
        torsion: i32,
        speed: i32,
        animation: Animation,
        left: bool,
        tower_level: i32,
    ) -> Self {
        Self {
            target_id: target.id,
            kind: MovementKind::Translation,
            direction,
            torsion: if direction.parallel_to(target.direction) {
                0
            } else {
                torsion
            },
            remaining: duration(speed),
            speed,
            from: Direction::None,
            to: Direction::None,
            animation,
            left,
            tower_level,
            pure_direct_force: false,
        }
    }

    pub fn rotation(
        target: &Entity,
        from: Direction,
        to: Direction,
        animation: Animation,
        speed: i32,
    ) -> Self {
        Self {
            target_id: target.id,
            kind: MovementKind::Rotation,
            direction: Direction::None,
            torsion: 0,
            remaining: duration(speed),
            speed,
            from,
            to,
            animation,
            left: to.left_of(from),
            tower_level: -1,
            pure_direct_force: false,
        }
    }

    pub fn pivot(
        target: &Entity,
        direction: Direction,
        from: Direction,
        to: Direction,
        animation: Animation,
        speed: i32,
    ) -> Self {
        Self {
            target_id: target.id,
            kind: MovementKind::Pivot,
            direction,
            torsion: 0,
            remaining: duration(speed),
            speed,
            from,
            to,
            animation,
            left: to.left_of(from),
            tower_level: -1,
            pure_direct_force: false,
        }
    }

    pub fn fixed(target: &Entity, speed: i32) -> Self {
        Self {
            target_id: target.id,
            kind: MovementKind::None,
            direction: Direction::None,
            torsion: 0,
            remaining: duration(speed),
            speed,
            from: Direction::None,
            to: Direction::None,
            animation: Animation::Fixed,
            left: false,
            tower_level: -1,
            pure_direct_force: false,
        }
    }

    pub fn set_speed(&mut self, speed: i32) {
        self.speed = speed;
        self.remaining = duration(speed);
    }

    pub fn tick(&mut self, elapsed: Fraction) {
        self.remaining = self.remaining - elapsed;
    }

    pub fn done(self) -> bool {
        self.remaining == Fraction::ZERO
    }

    pub fn starting(self) -> bool {
        self.remaining.numerator * i64::from(1 << (self.speed - 1)) == self.remaining.denominator
    }

    pub fn effective_direction(self) -> Direction {
        match self.kind {
            MovementKind::None => Direction::None,
            MovementKind::Rotation => Direction::continue_rotation(self.from, self.to),
            MovementKind::Translation | MovementKind::Pivot => self.direction,
        }
    }

    pub fn position(self) -> Fraction {
        Fraction::new(
            self.remaining.denominator - self.remaining.numerator * i64::from(self.speed),
            self.remaining.denominator,
        )
    }

    pub fn resolve(self, target: &mut Entity) {
        debug_assert_eq!(self.target_id, target.id);
        match self.kind {
            MovementKind::None => {}
            MovementKind::Translation => target.pos = target.pos + self.direction,
            MovementKind::Rotation => target.direction = self.to,
            MovementKind::Pivot => {
                target.pos = target.pos + self.direction;
                target.direction = self.to;
            }
        }
    }
}

fn duration(speed: i32) -> Fraction {
    assert!(speed >= 1, "movement speed must be positive");
    Fraction::new(1, i64::from(1 << (speed - 1)))
}

pub fn occupancy_for(
    entity: &Entity,
    movement: Option<Movement>,
    player_has_fork: bool,
) -> Vec<Occupancy> {
    let footprint = entity.footprint(player_has_fork);
    let Some(movement) = movement.filter(|movement| movement.kind != MovementKind::None) else {
        return footprint
            .into_iter()
            .map(|pos| Occupancy::new(pos, Direction::None, true, Fraction::ZERO, 0))
            .collect();
    };
    let position = movement.position();
    match movement.kind {
        MovementKind::Translation => footprint
            .into_iter()
            .flat_map(|pos| {
                [
                    Occupancy::new(pos, movement.direction, false, position, movement.speed),
                    Occupancy::new(
                        pos + movement.direction,
                        movement.direction,
                        true,
                        position,
                        movement.speed,
                    ),
                ]
            })
            .collect(),
        MovementKind::Rotation if footprint.len() == 2 => {
            let arc_direction = if movement.from.is_orthogonal() {
                Direction::continue_rotation(movement.from, movement.to)
            } else {
                Direction::continue_rotation(
                    movement.to,
                    Direction::continue_rotation(movement.from, movement.to),
                )
            };
            vec![
                Occupancy::new(entity.pos, Direction::None, false, position, movement.speed),
                Occupancy::new(
                    entity.pos + movement.from,
                    arc_direction,
                    false,
                    position,
                    movement.speed,
                ),
                Occupancy::new(
                    entity.pos + movement.to,
                    arc_direction,
                    true,
                    position,
                    movement.speed,
                ),
            ]
        }
        MovementKind::Rotation => vec![Occupancy::new(
            entity.pos,
            Direction::None,
            true,
            Fraction::ZERO,
            0,
        )],
        MovementKind::Pivot if footprint.len() == 2 => pivot_occupancy(entity, movement, position),
        MovementKind::Pivot | MovementKind::None => vec![Occupancy::new(
            entity.pos,
            Direction::None,
            true,
            Fraction::ZERO,
            0,
        )],
    }
}

fn pivot_occupancy(entity: &Entity, movement: Movement, position: Fraction) -> Vec<Occupancy> {
    let occupancy =
        |pos, direction, entering, speed| Occupancy::new(pos, direction, entering, position, speed);
    if movement.animation == Animation::TurnIn {
        if movement.from == movement.direction {
            return vec![
                Occupancy::new(
                    entity.pos + movement.direction,
                    Direction::None,
                    true,
                    Fraction::ZERO,
                    0,
                ),
                occupancy(
                    entity.pos,
                    movement.direction.inverse(),
                    false,
                    movement.speed,
                ),
                occupancy(
                    entity.pos + movement.direction + movement.to,
                    movement.direction,
                    true,
                    movement.speed,
                ),
            ];
        }
        if movement.from == movement.direction.inverse() {
            let turn = Direction::continue_rotation(movement.from, movement.to);
            return vec![
                occupancy(entity.pos, turn, false, movement.speed),
                occupancy(
                    entity.pos + movement.from,
                    movement.direction,
                    false,
                    movement.speed,
                ),
                occupancy(entity.pos + turn, movement.direction, true, movement.speed),
                occupancy(
                    entity.pos + movement.direction,
                    movement.direction,
                    true,
                    movement.speed,
                ),
            ];
        }
        if Direction::rotation_between(movement.from, movement.direction) == Some(movement.to) {
            return vec![
                occupancy(entity.pos, movement.direction, false, movement.speed),
                occupancy(
                    entity.pos + movement.direction,
                    movement.direction,
                    true,
                    movement.speed,
                ),
                occupancy(
                    entity.pos + movement.to,
                    movement.from,
                    true,
                    movement.speed + 1,
                ),
                occupancy(
                    entity.pos + movement.to + movement.direction,
                    movement.direction,
                    true,
                    movement.speed,
                ),
            ];
        }
        return vec![
            Occupancy::new(
                entity.pos + entity.direction,
                Direction::None,
                true,
                Fraction::ZERO,
                0,
            ),
            occupancy(entity.pos, movement.direction, false, movement.speed),
            occupancy(
                entity.pos + movement.direction,
                movement.direction,
                true,
                movement.speed,
            ),
        ];
    }
    if movement.to == movement.direction {
        let turn = Direction::continue_rotation(movement.to, movement.from).inverse();
        return vec![
            occupancy(entity.pos, movement.direction, false, movement.speed),
            occupancy(entity.pos + movement.direction, turn, true, movement.speed),
            occupancy(
                entity.pos + movement.direction.delta().scale(2),
                movement.direction,
                true,
                movement.speed,
            ),
            occupancy(
                entity.pos - turn.delta() + movement.direction.delta(),
                movement.direction,
                false,
                movement.speed,
            ),
            occupancy(
                entity.pos - turn.delta() + movement.direction.delta().scale(2),
                turn,
                false,
                movement.speed,
            ),
        ];
    }
    if movement.to == movement.direction.inverse() {
        return vec![
            Occupancy::new(entity.pos, Direction::None, true, Fraction::ZERO, 0),
            occupancy(
                entity.pos + movement.direction,
                movement.direction,
                true,
                movement.speed,
            ),
            occupancy(
                entity.pos + movement.from,
                movement.direction,
                false,
                movement.speed,
            ),
        ];
    }
    if Direction::continue_rotation(movement.to, movement.from) == movement.direction {
        return vec![
            Occupancy::new(
                entity.pos + entity.direction,
                Direction::None,
                true,
                Fraction::ZERO,
                0,
            ),
            occupancy(entity.pos, movement.direction, false, movement.speed),
            occupancy(
                entity.pos + movement.direction,
                movement.direction,
                true,
                movement.speed,
            ),
        ];
    }
    vec![
        occupancy(entity.pos, movement.direction, false, movement.speed),
        occupancy(
            entity.pos + movement.to,
            movement.to,
            true,
            movement.speed + 1,
        ),
        occupancy(
            entity.pos + movement.direction,
            movement.direction,
            true,
            movement.speed,
        ),
        occupancy(
            entity.pos + movement.direction + movement.to,
            movement.direction,
            true,
            movement.speed,
        ),
    ]
}

pub fn try_rotate(entity: &mut Entity, force_direction: Direction) {
    if !entity.direction.is_valid() || !force_direction.parallel_to(entity.direction) {
        entity.rotation = 1 - entity.rotation;
    }
}

pub fn can_roll(entity_type: EntityType) -> bool {
    entity_type == EntityType::Sausage
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sausage() -> Entity {
        Entity::parse("3,4,0,3,42,3,M;;,-1,0,0,0,8,0,0,0,").expect("valid sausage")
    }

    #[test]
    fn fractions_compare_by_value_without_eager_reduction() {
        assert_eq!(Fraction::new(1, 2), Fraction::new(2, 4));
        assert_eq!(
            Fraction::new(1, 2) + Fraction::new(1, 3),
            Fraction::new(5, 6)
        );
        assert!(Fraction::new(3, 4) > Fraction::new(2, 3));
    }

    #[test]
    fn occupancy_allows_a_same_direction_handoff() {
        let leaving = Occupancy::new(Coord::ZERO, Direction::East, false, Fraction::ZERO, 1);
        let entering = Occupancy::new(Coord::ZERO, Direction::East, true, Fraction::ZERO, 1);
        assert!(leaving.compatible_with(entering));
        assert!(!leaving.overlaps(entering));
        assert!(leaving.overlaps(Occupancy {
            direction: Direction::West,
            ..entering
        }));
    }

    #[test]
    fn translation_occupancy_tracks_both_sausage_cells() {
        let entity = sausage();
        let movement =
            Movement::translation(&entity, Direction::North, -666, 1, Animation::Idle, true, 0);
        let occupancy = occupancy_for(&entity, Some(movement), true);
        assert_eq!(occupancy.len(), 4);
        assert_eq!(occupancy.iter().filter(|item| item.entering).count(), 2);
        assert_eq!(movement.torsion, -666);
    }

    #[test]
    fn resolving_pivot_moves_anchor_and_changes_direction() {
        let mut entity = sausage();
        let movement = Movement::pivot(
            &entity,
            Direction::East,
            Direction::North,
            Direction::NorthEast,
            Animation::TurnIn,
            1,
        );
        movement.resolve(&mut entity);
        assert_eq!(entity.pos, Coord::new(4, 4, 0));
        assert_eq!(entity.direction, Direction::NorthEast);
    }
}
