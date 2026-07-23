use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{Direction, Session};

pub const API_VERSION: &str = "sokoban-api-v1";

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
    Move {
        directions: Vec<Direction>,
    },
    Undo {
        #[serde(default = "one")]
        steps: usize,
    },
    Reset,
    Submit,
}

const fn one() -> usize {
    1
}

impl Command {
    pub const fn name(&self) -> &'static str {
        match self {
            Self::Show => "show",
            Self::Levels { .. } => "levels",
            Self::Select { .. } => "select",
            Self::Move { .. } => "move",
            Self::Undo { .. } => "undo",
            Self::Reset => "reset",
            Self::Submit => "submit",
        }
    }

    pub const fn is_game_action(&self) -> bool {
        matches!(
            self,
            Self::Select { .. } | Self::Move { .. } | Self::Undo { .. } | Self::Reset
        )
    }
}

pub fn execute(session: &mut Session<'_>, command: &Command) -> Value {
    execute_with_observation(session, command, false).0
}

pub fn execute_observed(session: &mut Session<'_>, command: &Command) -> (Value, Vec<Value>) {
    execute_with_observation(session, command, true)
}

fn execute_with_observation(
    session: &mut Session<'_>,
    command: &Command,
    observe: bool,
) -> (Value, Vec<Value>) {
    match execute_inner(session, command, observe) {
        Ok((data, steps)) => (
            json!({
                "api_version": API_VERSION,
                "ok": true,
                "command": command.name(),
                "data": data,
            }),
            steps,
        ),
        Err(message) => (
            json!({
                "api_version": API_VERSION,
                "ok": false,
                "command": command.name(),
                "error": {
                    "code": "command_failed",
                    "message": message,
                },
                "state": session.snapshot(),
            }),
            Vec::new(),
        ),
    }
}

fn execute_inner(
    session: &mut Session<'_>,
    command: &Command,
    observe: bool,
) -> Result<(Value, Vec<Value>), String> {
    let data = match command {
        Command::Show => state(session),
        Command::Levels { tier } => Ok(json!({
            "levels": session.levels(tier.as_deref())?,
            "state": session.snapshot(),
        })),
        Command::Select { level } => {
            session.select(level)?;
            state(session)
        }
        Command::Move { directions } => {
            let score_before = session.score();
            let (results, newly_solved, snapshots) = if observe {
                session.move_many_observed(directions)?
            } else {
                let (results, newly_solved) = session.move_many(directions)?;
                (results, newly_solved, Vec::new())
            };
            let steps = observed_steps(
                snapshots,
                results
                    .iter()
                    .map(|result| json!({"command": "move", "direction": result.direction})),
                results.iter().map(|result| {
                    json!({
                        "ok": true,
                        "moved": result.moved,
                        "pushed": result.pushed,
                        "solved": result.solved,
                    })
                }),
                score_before,
            );
            return Ok((
                json!({
                    "steps": results,
                    "newly_solved": newly_solved,
                    "state": session.snapshot(),
                }),
                steps,
            ));
        }
        Command::Undo { steps } => {
            let score_before = session.score();
            let (undone, snapshots) = if observe {
                session.undo_observed(*steps)?
            } else {
                (session.undo(*steps)?, Vec::new())
            };
            let steps = observed_steps(
                snapshots,
                std::iter::repeat_n(json!({"command": "undo"}), undone),
                std::iter::repeat_n(json!({"ok": true}), undone),
                score_before,
            );
            return Ok((
                json!({
                    "undone": undone,
                    "state": session.snapshot(),
                }),
                steps,
            ));
        }
        Command::Reset => {
            session.reset()?;
            state(session)
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
    }?;
    Ok((data, Vec::new()))
}

fn state(session: &Session<'_>) -> Result<Value, String> {
    serde_json::to_value(session.snapshot())
        .map_err(|error| format!("could not serialize state: {error}"))
}

fn observed_steps(
    snapshots: Vec<Value>,
    actions: impl Iterator<Item = Value>,
    results: impl Iterator<Item = Value>,
    score_before: usize,
) -> Vec<Value> {
    let mut previous_score = score_before as i64;
    snapshots
        .into_iter()
        .zip(actions)
        .zip(results)
        .enumerate()
        .map(|(index, ((state, action), result))| {
            let score = state["campaign"]["score"]
                .as_i64()
                .unwrap_or(previous_score);
            let score_delta = score - previous_score;
            previous_score = score;
            json!({
                "index": index + 1,
                "action": action,
                "state": state,
                "result": result,
                "score": score,
                "score_delta": score_delta,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use crate::Campaign;

    use super::*;

    #[test]
    fn invalid_command_does_not_mutate_state() {
        let campaign = Campaign::load(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("data/campaign/sokoban.json"),
        )
        .expect("campaign");
        let mut session = Session::new(&campaign);
        let response = execute(
            &mut session,
            &Command::Select {
                level: "sasquatch-001".into(),
            },
        );
        assert_eq!(response["ok"], false);
        assert_eq!(response["state"]["campaign"]["score"], 0);
        assert_eq!(response["state"]["tiers"][2]["status"], "locked");
    }
}
