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
    let result = execute_inner(session, command, observe);
    match result {
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
        Command::Show => serde_json::to_value(session.agent_snapshot()?)
            .map_err(|error| format!("could not serialize snapshot: {error}")),
        Command::Levels => Ok(json!({
            "levels": session.levels()?,
        })),
        Command::Move { directions } => {
            if directions.len() > MAX_MOVES_PER_REQUEST {
                return Err(format!(
                    "move accepts at most {MAX_MOVES_PER_REQUEST} directions"
                ));
            }
            let score_before = session.snapshot()?.campaign.score;
            let (result, snapshots) = if observe {
                session.move_many_observed(directions)?
            } else {
                (session.move_many(directions)?, Vec::new())
            };
            let steps = observed_steps(
                snapshots,
                directions
                    .iter()
                    .take(result.applied)
                    .map(|direction| json!({"command": "move", "direction": direction})),
                result
                    .accepted
                    .iter()
                    .map(|accepted| json!({"ok": true, "accepted": accepted})),
                score_before,
            )?;
            return Ok((
                serde_json::to_value(result)
                    .map_err(|error| format!("could not serialize move result: {error}"))?,
                steps,
            ));
        }
        Command::Undo { count } => {
            if *count == 0 || *count > MAX_MOVES_PER_REQUEST {
                return Err(format!(
                    "undo count must be between 1 and {MAX_MOVES_PER_REQUEST}"
                ));
            }
            let score_before = session.snapshot()?.campaign.score;
            let (undone, snapshots) = if observe {
                session.undo_observed(*count)?
            } else {
                (session.undo(*count)?, Vec::new())
            };
            let steps = observed_steps(
                snapshots,
                std::iter::repeat_n(json!({"command": "undo"}), undone),
                std::iter::repeat_n(json!({"ok": true}), undone),
                score_before,
            )?;
            return Ok((
                json!({
                    "undone": undone,
                    "state": session.agent_snapshot()?,
                }),
                steps,
            ));
        }
        Command::Restart => {
            session.restart()?;
            Ok(json!({"state": session.agent_snapshot()?}))
        }
        Command::Submit => {
            let snapshot = session.snapshot()?;
            Ok(json!({
                "score": snapshot.campaign.score,
                "total": snapshot.campaign.total,
                "complete": snapshot.campaign.complete,
            }))
        }
    }?;
    Ok((data, Vec::new()))
}

fn observed_steps(
    snapshots: Vec<crate::GameSnapshot>,
    actions: impl Iterator<Item = Value>,
    results: impl Iterator<Item = Value>,
    score_before: usize,
) -> Result<Vec<Value>, String> {
    let mut previous_score = i64::try_from(score_before).map_err(|_| "score does not fit i64")?;
    snapshots
        .into_iter()
        .zip(actions)
        .zip(results)
        .enumerate()
        .map(|(index, ((snapshot, action), result))| {
            let score =
                i64::try_from(snapshot.campaign.score).map_err(|_| "score does not fit i64")?;
            let score_delta = score - previous_score;
            previous_score = score;
            Ok(json!({
                "index": index + 1,
                "action": action,
                "state": snapshot,
                "result": result,
                "score": score,
                "score_delta": score_delta,
            }))
        })
        .collect()
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

    #[test]
    fn observed_batch_exposes_each_authoritative_instruction_without_changing_api_response() {
        let root = crate::data_root();
        let campaign =
            Campaign::load_gzip(root.join("campaign").join("merged_binary.gz")).expect("campaign");
        let entries =
            CampaignEntries::load(root.join("campaign").join("entries.tar.gz")).expect("entries");
        let mut session = Session::new(&campaign, &entries).expect("session");
        let (response, steps) = execute_observed(
            &mut session,
            &Command::Move {
                directions: vec![Direction::North, Direction::East],
            },
        );

        assert_eq!(response["ok"], true);
        assert_eq!(response["data"]["applied"], 2);
        assert!(response.get("steps").is_none());
        assert_eq!(steps.len(), 2);
        assert_eq!(steps[0]["action"]["direction"], "north");
        assert_eq!(steps[1]["action"]["direction"], "east");
        assert_eq!(steps[1]["state"]["overworld"]["actions"], 2);
    }

    #[test]
    fn agent_overworld_responses_do_not_repeat_the_global_map() {
        let root = crate::data_root();
        let campaign =
            Campaign::load_gzip(root.join("campaign").join("merged_binary.gz")).expect("campaign");
        let entries =
            CampaignEntries::load(root.join("campaign").join("entries.tar.gz")).expect("entries");
        let mut session = Session::new(&campaign, &entries).expect("session");

        let response = execute(&mut session, &Command::Show);
        let encoded = serde_json::to_vec(&response).expect("response JSON");
        let data = &response["data"];
        assert!(encoded.len() < 35_000, "{} bytes", encoded.len());
        assert_eq!(data["overworld_map"], Value::Null);
        assert!(data["overworld"]["entrances"].as_array().unwrap().len() < entries.levels.len());
        assert!(data["overworld"]["islands"].as_array().unwrap().len() < 205);

        let observer = session.observer_snapshot().expect("observer state");
        assert_eq!(observer.overworld.unwrap().entrances.len(), 86);
        assert_eq!(observer.overworld_map.unwrap().tiles.len(), 16_261);
    }
}
