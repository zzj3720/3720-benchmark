use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{Direction, Session};

pub const API_VERSION: &str = "sausage-api-v1";
pub const MAX_MOVES_PER_REQUEST: usize = 32;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "command", rename_all = "snake_case")]
pub enum Command {
    Show,
    Levels,
    Move { directions: Vec<Direction> },
    Undo { count: usize },
    Restart,
    Submit,
}

impl Command {
    pub const fn name(&self) -> &'static str {
        match self {
            Self::Show => "show",
            Self::Levels => "levels",
            Self::Move { .. } => "move",
            Self::Undo { .. } => "undo",
            Self::Restart => "restart",
            Self::Submit => "submit",
        }
    }
}

pub fn execute(session: &mut Session<'_>, command: &Command) -> Value {
    let result = execute_inner(session, command);
    match result {
        Ok(data) => json!({
            "api_version": API_VERSION,
            "ok": true,
            "command": command.name(),
            "data": data,
        }),
        Err(message) => json!({
            "api_version": API_VERSION,
            "ok": false,
            "command": command.name(),
            "error": {
                "code": "command_failed",
                "message": message,
            },
        }),
    }
}

fn execute_inner(session: &mut Session<'_>, command: &Command) -> Result<Value, String> {
    match command {
        Command::Show => serde_json::to_value(session.snapshot()?)
            .map_err(|error| format!("could not serialize snapshot: {error}")),
        Command::Levels => Ok(json!({
            "levels": session.levels()?,
            "state": session.snapshot()?,
        })),
        Command::Move { directions } => {
            if directions.len() > MAX_MOVES_PER_REQUEST {
                return Err(format!(
                    "move accepts at most {MAX_MOVES_PER_REQUEST} directions"
                ));
            }
            serde_json::to_value(session.move_many(directions)?)
                .map_err(|error| format!("could not serialize move result: {error}"))
        }
        Command::Undo { count } => {
            if *count == 0 || *count > MAX_MOVES_PER_REQUEST {
                return Err(format!(
                    "undo count must be between 1 and {MAX_MOVES_PER_REQUEST}"
                ));
            }
            Ok(json!({
                "undone": session.undo(*count)?,
                "state": session.snapshot()?,
            }))
        }
        Command::Restart => {
            session.restart()?;
            Ok(json!({"state": session.snapshot()?}))
        }
        Command::Submit => {
            let snapshot = session.snapshot()?;
            Ok(json!({
                "score": snapshot.campaign.score,
                "total": snapshot.campaign.total,
                "complete": snapshot.campaign.complete,
                "state": snapshot,
            }))
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{Campaign, CampaignEntries};

    use super::*;

    #[test]
    fn rejects_an_oversized_move_batch_without_changing_state() {
        let root = crate::data_root();
        let campaign =
            Campaign::load_gzip(root.join("campaign").join("merged_binary.gz")).expect("campaign");
        let entries =
            CampaignEntries::load(root.join("campaign").join("entries.tar.gz")).expect("entries");
        let mut session = Session::new(&campaign, &entries).expect("session");
        let response = execute(
            &mut session,
            &Command::Move {
                directions: vec![Direction::North; MAX_MOVES_PER_REQUEST + 1],
            },
        );
        assert_eq!(response["ok"], false);
        assert_eq!(session.record().histories[0].len(), 0);
    }
}
