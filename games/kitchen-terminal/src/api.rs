use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{Direction, Session};

pub const API_VERSION: &str = "overcooked-api-v1";

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "command", rename_all = "snake_case", deny_unknown_fields)]
pub enum Command {
    Show,
    Start,
    Move {
        direction: Direction,
    },
    Dash {
        direction: Direction,
    },
    Switch,
    Interact {
        target: String,
    },
    StartWork {
        target: String,
    },
    StopWork,
    SetAlarm {
        id: String,
        after_ms: u64,
        #[serde(default)]
        note: String,
    },
    CancelAlarm {
        id: String,
    },
    Wake,
    Submit,
}

impl Command {
    pub const fn name(&self) -> &'static str {
        match self {
            Self::Show => "show",
            Self::Start => "start",
            Self::Move { .. } => "move",
            Self::Dash { .. } => "dash",
            Self::Switch => "switch",
            Self::Interact { .. } => "interact",
            Self::StartWork { .. } => "start_work",
            Self::StopWork => "stop_work",
            Self::SetAlarm { .. } => "set_alarm",
            Self::CancelAlarm { .. } => "cancel_alarm",
            Self::Wake => "wake",
            Self::Submit => "submit",
        }
    }

    pub const fn is_game_action(&self) -> bool {
        matches!(
            self,
            Self::Start
                | Self::Move { .. }
                | Self::Dash { .. }
                | Self::Switch
                | Self::Interact { .. }
                | Self::StartWork { .. }
                | Self::StopWork
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
        Command::Start => {
            session.start()?;
            state(session)
        }
        Command::Move { direction } => {
            session.move_chef(*direction, false)?;
            state(session)
        }
        Command::Dash { direction } => {
            session.move_chef(*direction, true)?;
            state(session)
        }
        Command::Switch => {
            session.switch()?;
            state(session)
        }
        Command::Interact { target } => {
            session.interact(target)?;
            state(session)
        }
        Command::StartWork { target } => {
            let expected_done_ms = session.start_work(target)?;
            Ok(json!({
                "expected_done_ms": expected_done_ms,
                "state": session.snapshot(),
            }))
        }
        Command::StopWork => {
            session.stop_work()?;
            state(session)
        }
        Command::SetAlarm { id, after_ms, note } => {
            let due_ms = session.set_alarm(id, *after_ms, note)?;
            Ok(json!({
                "due_ms": due_ms,
                "state": session.snapshot(),
            }))
        }
        Command::CancelAlarm { id } => {
            session.cancel_alarm(id)?;
            state(session)
        }
        Command::Wake => {
            let alarms = session.deliver_due_alarms();
            Ok(json!({
                "alarms": alarms,
                "state": session.snapshot(),
            }))
        }
        Command::Submit => {
            let snapshot = session.snapshot();
            Ok(json!({
                "score": snapshot.campaign.score,
                "stars": snapshot.campaign.stars,
                "complete": snapshot.shift.status == "complete",
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
    use super::*;

    #[test]
    fn command_schema_rejects_batches_and_client_timestamps() {
        assert!(
            serde_json::from_str::<Command>(r#"{"command":"move","direction":"north"}"#).is_ok()
        );
        assert!(
            serde_json::from_str::<Command>(
                r#"{"command":"move","direction":"north","at_ms":1000}"#
            )
            .is_err()
        );
        assert!(
            serde_json::from_str::<Command>(r#"{"command":"interact","target":["a","b"]}"#)
                .is_err()
        );
    }
}
