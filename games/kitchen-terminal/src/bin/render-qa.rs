use std::path::Path;

use kitchen_terminal::{GameData, Session, SessionConfig};
use serde_json::{Value, json};

fn main() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("data/overcooked-1");
    let levels = [
        (1, "Tutorial salad"),
        (2, "Soup kitchen"),
        (5, "Burger kitchen"),
        (10, "Conveyor kitchen"),
        (16, "Moving counters"),
        (20, "Shuttle switches"),
        (21, "Hell hazards"),
        (30, "Final service"),
    ];
    let samples = levels
        .into_iter()
        .map(|(level, title)| {
            let data = GameData::load(&root, level).expect("imported kitchen");
            let mut session = Session::new(
                &data,
                SessionConfig {
                    time_scale: 1,
                    seed: 448_510 + u64::from(level),
                },
            )
            .expect("session");
            session.start().expect("start");
            session.advance_to(5_000).expect("sample clock");
            sample(&session, level, title)
        })
        .collect::<Vec<_>>();
    println!(
        "{}",
        serde_json::to_string_pretty(&json!({
            "schema": "kitchen-render-qa-v1",
            "samples": samples,
        }))
        .expect("serialize QA")
    );
}

fn sample(session: &Session<'_>, level: u8, title: &str) -> Value {
    let state = session.snapshot();
    json!({
        "reference": format!("kitchen-{level:02}"),
        "title": title,
        "area": state.campaign.scene,
        "step": state.shift.elapsed_ms,
        "total_steps": state.shift.duration_ms,
        "features": {},
        "state": state,
    })
}
