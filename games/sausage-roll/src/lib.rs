//! Deterministic Rust adaptation of Stephen's Sausage Roll.
//!
//! The checked-in campaign is extracted from a locally owned copy of the game.
//! It is benchmark data, not a source-code dependency of the original engine.

use std::path::Path;

/// Repository-level development data. Packaged binaries receive explicit
/// runtime paths, while local tools and tests share this single authority.
pub fn data_root() -> &'static Path {
    Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/data"))
}

pub mod api;
pub mod campaign;
pub mod entries;
pub mod game3d;
pub mod model;
pub mod oracle;
pub mod physics;
pub mod replay;
pub mod session;
pub mod world;

pub use api::{API_VERSION, Command, MAX_MOVES_PER_REQUEST, execute};
pub use campaign::Campaign;
pub use entries::{CampaignEntries, CampaignEntry};
pub use game3d::Game3d;
pub use model::{Coord, Direction, Entity, EntityType, GameState};
pub use oracle::{
    EngineVerification, GuidedReplay, OracleCampaign, OracleCheckpoint, OracleEntity, OracleSegment,
};
pub use physics::{Animation, Fraction, Movement, MovementKind, Occupancy};
pub use replay::Replay;
pub use session::{
    CAMPAIGN_ID, CampaignStatus, EntityView, GameSnapshot, LevelStatus, MoveResult, Session,
    SessionRecord, TileView,
};
pub use world::PhysicsWorld;
