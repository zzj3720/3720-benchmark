use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{Cell, Session};

pub const API_VERSION: &str = "minesweeper-api-v2";

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "command", rename_all = "snake_case", deny_unknown_fields)]
pub enum Command {
    Show,
    Levels {
        #[serde(default)]
        tier: Option<String>,
    },
    Select {
        level: String,
    },
    Reveal {
        cells: Vec<Cell>,
    },
    Flag {
        cell: Cell,
    },
    Chord {
        cell: Cell,
    },
    Submit,
}

impl Command {
    pub const fn name(&self) -> &'static str {
        match self {
            Self::Show => "show",
            Self::Levels { .. } => "levels",
            Self::Select { .. } => "select",
            Self::Reveal { .. } => "reveal",
            Self::Flag { .. } => "flag",
            Self::Chord { .. } => "chord",
            Self::Submit => "submit",
        }
    }

    pub const fn is_game_action(&self) -> bool {
        matches!(
            self,
            Self::Select { .. } | Self::Reveal { .. } | Self::Flag { .. } | Self::Chord { .. }
        )
    }
}

pub fn execute(session: &mut Session<'_>, command: &Command) -> Value {
    match execute_inner(session, command) {
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
            "state": session.snapshot(),
        }),
    }
}

fn execute_inner(session: &mut Session<'_>, command: &Command) -> Result<Value, String> {
    match command {
        Command::Show => state(session),
        Command::Levels { tier } => Ok(json!({
            "levels": session.levels(tier.as_deref())?,
            "state": session.snapshot(),
        })),
        Command::Select { level } => {
            session.select(level)?;
            state(session)
        }
        Command::Reveal { cells } => {
            let (results, newly_passed_tier) = session.reveal(cells)?;
            Ok(json!({
                "steps": results,
                "newly_passed_tier": newly_passed_tier,
                "state": session.snapshot(),
            }))
        }
        Command::Flag { cell } => {
            let result = session.flag(*cell)?;
            Ok(json!({
                "result": result,
                "state": session.snapshot(),
            }))
        }
        Command::Chord { cell } => {
            let (result, newly_passed_tier) = session.chord(*cell)?;
            Ok(json!({
                "result": result,
                "newly_passed_tier": newly_passed_tier,
                "state": session.snapshot(),
            }))
        }
        Command::Submit => {
            let snapshot = session.snapshot();
            Ok(json!({
                "score": snapshot.campaign.score,
                "max_score": snapshot.campaign.max_score,
                "complete": snapshot.campaign.complete,
                "state": snapshot,
            }))
        }
    }
}

fn state(session: &Session<'_>) -> Result<Value, String> {
    serde_json::to_value(session.snapshot())
        .map_err(|error| format!("could not serialize state: {error}"))
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use crate::Campaign;

    use super::*;

    #[test]
    fn locked_level_failure_does_not_mutate_state() {
        let campaign = Campaign::load(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("data/campaign/minesweeper.json"),
        )
        .expect("campaign");
        let mut session = Session::new(&campaign);
        let response = execute(
            &mut session,
            &Command::Select {
                level: "expert-01".into(),
            },
        );
        assert_eq!(response["ok"], false);
        assert_eq!(response["state"]["campaign"]["score"], 0);
        assert_eq!(response["state"]["tiers"][3]["status"], "locked");
    }
}
