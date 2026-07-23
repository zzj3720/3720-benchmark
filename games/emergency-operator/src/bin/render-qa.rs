use std::path::Path;

use operator_terminal::{Campaign, Session};
use serde_json::{Value, json};

fn main() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let campaign =
        Campaign::load(root.join("data/campaign/911-career.json")).expect("compiled career");
    let mut session = Session::new(&campaign);
    let mut samples = Vec::new();

    session.start().expect("start");
    push(&mut samples, &session, "Duty ready");
    session.advance_to(30_000).expect("first call");
    push(&mut samples, &session, "First line ringing");

    let first_call = campaign
        .shift
        .calls
        .iter()
        .find(|event| event.kind == operator_terminal::campaign::EventKind::Phone)
        .expect("phone call");
    session.answer(&first_call.id).expect("answer first call");
    push(&mut samples, &session, "Live conversation");
    for _ in 0..2 {
        let Some(choice) = session
            .snapshot()
            .calls
            .iter()
            .find(|call| call.id == first_call.id)
            .and_then(|call| call.choices.first())
            .map(|choice| choice.id.clone())
        else {
            break;
        };
        session
            .say(&first_call.id, &choice)
            .expect("follow dialogue");
        push(&mut samples, &session, "Dialogue branch");
    }

    session.advance_to(75_000).expect("first report");
    push(&mut samples, &session, "Call and CAD report overlap");
    let reported = session
        .snapshot()
        .incidents
        .into_iter()
        .find(|incident| incident.status == "reported")
        .expect("reported incident");
    for requirement in &reported.requirements {
        let unit = campaign
            .shift
            .units
            .iter()
            .find(|unit| unit.role == requirement.role)
            .expect("unit for requirement");
        session
            .dispatch(&unit.id, &reported.id)
            .expect("dispatch response");
    }
    push(&mut samples, &session, "Units dispatched");
    session.advance_to(120_000).expect("overlap");
    push(&mut samples, &session, "Concurrent response");

    let timer_call = campaign
        .shift
        .calls
        .iter()
        .find(|event| event.id == "chapter-1-duty-1-call-2-81")
        .expect("small car fire call");
    let mut timer_session = Session::new(&campaign);
    timer_session.start().expect("start timer sample");
    timer_session
        .advance_to(timer_call.arrival_ms)
        .expect("timer call arrival");
    timer_session
        .answer(&timer_call.id)
        .expect("answer timer call");
    timer_session
        .say(&timer_call.id, "address")
        .expect("locate timer incident");
    push(
        &mut samples,
        &timer_session,
        "Live scene timer after location",
    );
    timer_session
        .advance_to(timer_call.arrival_ms + 30_000)
        .expect("advance scene timer");
    push(&mut samples, &timer_session, "Scene timer under pressure");

    println!(
        "{}",
        serde_json::to_string_pretty(&json!({
            "schema": "operator-render-qa-v1",
            "samples": samples,
        }))
        .expect("serialize QA")
    );
}

fn push(samples: &mut Vec<Value>, session: &Session<'_>, title: &str) {
    let state = session.snapshot();
    samples.push(json!({
        "reference": format!("operator-{:02}", samples.len() + 1),
        "title": title,
        "area": state.shift.duty.as_ref().map_or("Dispatch", |duty| duty.city.as_str()),
        "step": state.shift.elapsed_ms,
        "total_steps": state.shift.duration_ms,
        "features": {},
        "state": state,
    }));
}
