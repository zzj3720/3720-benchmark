use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::Session;

pub const API_VERSION: &str = "emergency-operator-api-v1";

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "command", rename_all = "snake_case", deny_unknown_fields)]
pub enum Command {
    Show,
    Start,
    Answer {
        call: String,
    },
    Say {
        call: String,
        choice: String,
    },
    Dispatch {
        unit: String,
        incident: String,
    },
    Recall {
        unit: String,
    },
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
            Self::Answer { .. } => "answer",
            Self::Say { .. } => "say",
            Self::Dispatch { .. } => "dispatch",
            Self::Recall { .. } => "recall",
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
                | Self::Answer { .. }
                | Self::Say { .. }
                | Self::Dispatch { .. }
                | Self::Recall { .. }
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
        Command::Answer { call } => {
            session.answer(call)?;
            state(session)
        }
        Command::Say { call, choice } => {
            session.say(call, choice)?;
            state(session)
        }
        Command::Dispatch { unit, incident } => {
            let arrival_ms = session.dispatch(unit, incident)?;
            Ok(json!({
                "arrival_ms": arrival_ms,
                "state": session.snapshot(),
            }))
        }
        Command::Recall { unit } => {
            let available_ms = session.recall(unit)?;
            Ok(json!({
                "available_ms": available_ms,
                "state": session.snapshot(),
            }))
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
                "max_score": snapshot.campaign.max_score,
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
    use std::path::Path;

    use crate::Campaign;

    use super::*;

    #[test]
    fn action_schema_has_no_batch_or_future_timestamp() {
        assert!(serde_json::from_str::<Command>(r#"{"command":"answer","call":"c"}"#).is_ok());
        assert!(
            serde_json::from_str::<Command>(r#"{"command":"answer","call":"c","at_ms":1000}"#)
                .is_err()
        );
        assert!(
            serde_json::from_str::<Command>(
                r#"{"command":"dispatch","unit":["u1","u2"],"incident":"i"}"#
            )
            .is_err()
        );
    }

    #[test]
    fn failed_action_returns_the_current_state() {
        let campaign =
            Campaign::load(Path::new(env!("CARGO_MANIFEST_DIR")).join("data/campaign/pilot.json"))
                .expect("campaign");
        let mut session = Session::new(&campaign);
        let response = execute(
            &mut session,
            &Command::Answer {
                call: "medical-1".to_owned(),
            },
        );
        assert_eq!(response["ok"], false);
        assert_eq!(response["state"]["shift"]["status"], "not_started");
    }
}
