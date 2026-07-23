use std::fs;
use std::path::Path;

use serde_json::Value;

fn read_json(path: &Path) -> Value {
    serde_json::from_slice(&fs::read(path).expect("read imported data"))
        .expect("parse imported data")
}

#[test]
fn imported_911_operator_content_is_complete_and_self_consistent() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("data/911-operator");
    let manifest = read_json(&root.join("manifest.json"));
    let inventory = &manifest["inventory"];
    assert_eq!(inventory["calls"], 56);
    assert_eq!(inventory["call_dialogue_nodes"], 2_159);
    assert_eq!(inventory["call_scene_elements"], 247);
    assert_eq!(inventory["internal_conversations"], 12);
    assert_eq!(inventory["report_types"], 137);
    assert_eq!(inventory["vehicle_types"], 28);
    assert_eq!(inventory["maps"], 30);
    assert_eq!(inventory["career_chapters"], 5);
    assert_eq!(inventory["career_duties"], 14);
    assert_eq!(inventory["career_fixed_call_slots"], 60);

    let calls = read_json(&root.join("calls/index.json"));
    for call in calls["calls"].as_array().expect("calls") {
        assert!(
            root.join(call["file"].as_str().expect("call file"))
                .is_file()
        );
    }

    let maps = read_json(&root.join("maps/index.json"));
    for map in maps["maps"].as_array().expect("maps") {
        assert!(root.join(map["file"].as_str().expect("map file")).is_file());
    }

    let campaign = read_json(&root.join("campaign.json"));
    let chapters = campaign["chapters"].as_array().expect("chapters");
    assert_eq!(chapters[0]["city"], "Kapolei");
    assert_eq!(chapters[4]["city"], "Washington");
    assert_eq!(
        chapters[4]["duties"][3]["fixed_calls"]
            .as_array()
            .expect("final duty calls")
            .len(),
        0,
        "Washington's fourth duty intentionally contains only generated reports"
    );
}
