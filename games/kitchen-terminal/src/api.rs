use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::Session;
use crate::engine::{DestinationView, ObjectView};

pub const API_VERSION: &str = "overcooked-api-v4";
pub const DYNAMIC_STATE_SCHEMA: &str = "overcooked-dynamic-state-v3";

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "command", rename_all = "snake_case", deny_unknown_fields)]
pub enum Command {
    Show,
    Start,
    Go {
        target: String,
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
            Self::Go { .. } => "go",
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
                | Self::Go { .. }
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
            "state": dynamic_state(session),
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
        Command::Go { target } => {
            let expected_arrival_ms = session.go(target)?;
            Ok(json!({
                "expected_arrival_ms": expected_arrival_ms,
                "state": dynamic_state(session),
            }))
        }
        Command::Switch => {
            session.switch()?;
            dynamic_state_value(session)
        }
        Command::Interact { target } => {
            session.interact(target)?;
            dynamic_state_value(session)
        }
        Command::StartWork { target } => {
            let expected_done_ms = session.start_work(target)?;
            Ok(json!({
                "expected_done_ms": expected_done_ms,
                "state": dynamic_state(session),
            }))
        }
        Command::StopWork => {
            session.stop_work()?;
            dynamic_state_value(session)
        }
        Command::SetAlarm { id, after_ms, note } => {
            let due_ms = session.set_alarm(id, *after_ms, note)?;
            Ok(json!({
                "due_ms": due_ms,
                "state": dynamic_state(session),
            }))
        }
        Command::CancelAlarm { id } => {
            session.cancel_alarm(id)?;
            dynamic_state_value(session)
        }
        Command::Wake => {
            let alarms = session.deliver_due_alarms();
            Ok(json!({
                "alarms": alarms,
                "state": dynamic_state(session),
            }))
        }
        Command::Submit => {
            let snapshot = session.snapshot();
            Ok(json!({
                "score": snapshot.campaign.score,
                "stars": snapshot.campaign.stars,
                "complete": snapshot.shift.status == "complete",
                "state": dynamic_state(session),
            }))
        }
    }
}

fn state(session: &Session<'_>) -> Result<Value, String> {
    let snapshot = session.snapshot();
    Ok(json!({
        "schema": DYNAMIC_STATE_SCHEMA,
        "static_map_omitted": true,
        "campaign": snapshot.campaign,
        "shift": snapshot.shift,
        "active_chef": snapshot.active_chef,
        "chefs": snapshot.chefs,
        "destinations": model_destinations(snapshot.destinations),
        "orders": snapshot.orders,
        "map": {
            "objects_are_sparse": true,
            "objects": model_objects(snapshot.map.objects),
            "systems": snapshot.map.systems.into_iter().filter(|system| {
                system.active || system.progress.is_some()
            }).collect::<Vec<_>>(),
        },
        "works": snapshot.works,
        "hazards": snapshot.hazards,
        "alarms": snapshot.alarms,
        "recent_events": snapshot.recent_events,
        "controls": snapshot.controls,
    }))
}

fn dynamic_state_value(session: &Session<'_>) -> Result<Value, String> {
    Ok(dynamic_state(session))
}

fn dynamic_state(session: &Session<'_>) -> Value {
    let snapshot = session.snapshot();
    json!({
        "schema": DYNAMIC_STATE_SCHEMA,
        "static_map_omitted": true,
        "campaign": {
            "score": snapshot.campaign.score,
            "stars": snapshot.campaign.stars,
        },
        "shift": snapshot.shift,
        "active_chef": snapshot.active_chef,
        "chefs": snapshot.chefs,
        "destinations": model_destinations(snapshot.destinations),
        "orders": snapshot.orders,
        "map": {
            "objects_are_sparse": true,
            "objects": snapshot.map.objects.into_iter().filter(|object| {
                object.item.is_some()
                    || object.plate_count.unwrap_or_default() > 0
                    || object.dirty_plate_count.unwrap_or_default() > 0
                    || object.enabled.is_some()
                    || object.fire_strength.is_some()
            }).map(|object| json!({
                "id": object.id,
                "item": object.item,
                "plate_count": object.plate_count,
                "dirty_plate_count": object.dirty_plate_count,
                "enabled": object.enabled,
                "fire_strength": object.fire_strength,
            })).collect::<Vec<_>>(),
            "systems": snapshot.map.systems.into_iter().filter(|system| {
                system.active || system.progress.is_some()
            }).map(|system| json!({
                "id": system.id,
                "world": system.world,
                "target": system.target,
                "active": system.active,
                "progress": system.progress,
            })).collect::<Vec<_>>(),
        },
        "works": snapshot.works,
        "hazards": snapshot.hazards,
        "alarms": snapshot.alarms,
        "recent_events": snapshot.recent_events,
    })
}

fn model_destinations(destinations: Vec<DestinationView>) -> Vec<Value> {
    destinations
        .into_iter()
        .map(|destination| {
            json!({
                "target": destination.target,
                "name": destination.name,
                "kind": destination.kind,
                "travel_ms": destination.travel_ms,
            })
        })
        .collect()
}

fn model_objects(objects: Vec<ObjectView>) -> Vec<Value> {
    objects
        .into_iter()
        .filter(|object| {
            object.item.is_some()
                || object.supply.is_some()
                || object.processes_to.is_some()
                || object.plate_count.unwrap_or_default() > 0
                || object.dirty_plate_count.unwrap_or_default() > 0
                || object.enabled.is_some()
                || object.fire_strength.is_some()
        })
        .map(|object| {
            json!({
                "id": object.id,
                "name": object.name,
                "kind": object.kind,
                "item": object.item,
                "supply": object.supply,
                "processes_to": object.processes_to,
                "plate_count": object.plate_count,
                "dirty_plate_count": object.dirty_plate_count,
                "enabled": object.enabled,
                "fire_strength": object.fire_strength,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_schema_rejects_batches_and_client_timestamps() {
        assert!(
            serde_json::from_str::<Command>(r#"{"command":"move","direction":"north"}"#).is_err()
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
        assert!(serde_json::from_str::<Command>(r#"{"command":"go","target":"board"}"#).is_ok());
    }

    #[test]
    fn dynamic_state_does_not_repeat_static_map_topology() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let data = crate::GameData::load(root.join("data/overcooked-1"), 1).expect("game data");
        let mut session = Session::new(&data, crate::SessionConfig::default()).expect("session");
        let started = execute(&mut session, &Command::Start);
        assert_eq!(started["data"]["schema"], DYNAMIC_STATE_SCHEMA);
        assert!(started["data"]["map"].get("walkable").is_none());
        assert_eq!(started["data"]["map"]["objects_are_sparse"], true);
        assert!(started["data"]["shift"].get("time_scale").is_none());
        assert!(started["data"]["destinations"][0].get("position").is_none());
        assert!(started["data"]["destinations"][0].get("steps").is_none());
        assert!(
            started["data"]["map"]["objects"]
                .as_array()
                .expect("relevant objects")
                .iter()
                .all(|object| object["kind"] != "structure")
        );

        let target = started["data"]["destinations"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|destination| destination["travel_ms"].as_u64().unwrap_or(0) > 0)
            .and_then(|destination| destination["target"].as_str())
            .expect("non-local destination")
            .to_owned();
        let travelled = execute(&mut session, &Command::Go { target });
        let state = &travelled["data"]["state"];
        assert_eq!(state["schema"], DYNAMIC_STATE_SCHEMA);
        assert!(state["map"].get("walkable").is_none());
        assert_eq!(state["map"]["objects_are_sparse"], true);
        assert!(state["destinations"].is_array());
        assert!(
            state["map"]["objects"]
                .as_array()
                .expect("dynamic objects")
                .len()
                < started["data"]["map"]["objects"]
                    .as_array()
                    .expect("static objects")
                    .len()
        );
        assert!(state["shift"].get("time_scale").is_none());
        assert!(state["controls"].is_null());
    }
}
