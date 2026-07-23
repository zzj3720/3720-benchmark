use std::env;
use std::path::PathBuf;

use operator_terminal::{Campaign, Role, Session};

fn main() {
    if let Err(error) = run() {
        eprintln!("operator-reference-run: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let path = env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join("data/campaign/911-career.json"));
    let campaign = Campaign::load(path)?;
    let mut session = Session::new(&campaign);
    session.start()?;

    while session.elapsed_ms() < session.duration_ms() {
        handle_calls(&campaign, &mut session)?;
        dispatch_idle_units(&mut session)?;
        session.advance_to(
            session
                .elapsed_ms()
                .saturating_add(1_000)
                .min(session.duration_ms()),
        )?;
    }

    let snapshot = session.snapshot();
    for status in ["resolved", "lost", "reported", "hidden"] {
        let count = snapshot
            .incidents
            .iter()
            .filter(|incident| incident.status == status)
            .count();
        println!("{status}: {count}");
    }
    for status in ["completed", "missed", "dropped"] {
        let count = snapshot
            .calls
            .iter()
            .filter(|call| call.status == status)
            .count();
        println!("{status}_calls: {count}");
    }
    println!("score: {}", snapshot.campaign.score);
    println!("ceiling: {}", snapshot.campaign.max_score);
    Ok(())
}

fn handle_calls(campaign: &Campaign, session: &mut Session<'_>) -> Result<(), String> {
    loop {
        let snapshot = session.snapshot();
        let Some(call) = snapshot.calls.iter().find(|call| call.status == "ringing") else {
            return Ok(());
        };
        let call_id = call.id.clone();
        session.answer(&call_id)?;
        for _ in 0..10_000 {
            let snapshot = session.snapshot();
            let call = snapshot
                .calls
                .iter()
                .find(|call| call.id == call_id)
                .expect("active call remains in the snapshot");
            if call.status != "active" {
                break;
            }
            let definition = campaign
                .shift
                .calls
                .iter()
                .find(|call| call.id == call_id)
                .expect("snapshot call has a definition");
            let reported = definition.incident.as_ref().is_some_and(|incident| {
                snapshot
                    .incidents
                    .iter()
                    .any(|view| view.id == incident.id && view.status != "hidden")
            });
            let choice = call
                .choices
                .iter()
                .max_by_key(|choice| {
                    let node = definition
                        .node(&choice.id)
                        .expect("choice has a dialogue node");
                    let actions = node.actions.join(";").to_ascii_lowercase();
                    let location = actions.contains("actionsetlocation");
                    let hangup = actions.contains("actionhangup");
                    (
                        (!reported && location) as u8,
                        (!hangup) as u8,
                        action_value(&actions),
                        node.aar.len(),
                    )
                })
                .ok_or_else(|| format!("active call {call_id:?} has no choices"))?
                .id
                .clone();
            session.say(&call_id, &choice)?;
        }
    }
}

fn action_value(actions: &str) -> i64 {
    actions
        .split(';')
        .filter_map(|action| {
            let action = action
                .chars()
                .filter(|c| !c.is_whitespace())
                .collect::<String>();
            let assignment = action.strip_prefix("opinioneffect")?;
            let value = assignment
                .trim_start_matches(['+', '-', '='])
                .parse::<f64>()
                .ok()?;
            let sign = if assignment.starts_with("-=") {
                -1.0
            } else {
                1.0
            };
            Some((sign * value * 10.0).round() as i64)
        })
        .sum()
}

fn dispatch_idle_units(session: &mut Session<'_>) -> Result<(), String> {
    loop {
        let snapshot = session.snapshot();
        let Some(unit) = snapshot.units.iter().find(|unit| unit.status == "idle") else {
            return Ok(());
        };
        let incident = snapshot
            .incidents
            .iter()
            .filter(|incident| incident.status == "reported")
            .filter(|incident| {
                incident.requirements.iter().any(|requirement| {
                    requirement.role == unit.role && requirement.remaining_work_ms > 0
                })
            })
            .max_by_key(|incident| urgency(incident, unit.role));
        let Some(incident) = incident else {
            return Ok(());
        };
        let unit_id = unit.id.clone();
        let incident_id = incident.id.clone();
        session.dispatch(&unit_id, &incident_id)?;
    }
}

fn urgency(incident: &operator_terminal::IncidentView, role: Role) -> (u64, u64) {
    let remaining = incident
        .requirements
        .iter()
        .find(|requirement| requirement.role == role)
        .map_or(0, |requirement| requirement.remaining_work_ms);
    let health_pressure = if incident.health_decay_milli_per_minute > 0 {
        (incident.health_decay_milli_per_minute as u64).saturating_mul(1_000_000)
            / incident.health_milli.max(1) as u64
    } else {
        0
    };
    (health_pressure, remaining)
}
