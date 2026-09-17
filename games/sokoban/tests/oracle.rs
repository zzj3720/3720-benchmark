use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use sokoban_benchmark::{Campaign, Direction, Session};

#[test]
fn complete_oracle_replays_all_305_levels() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let campaign =
        Campaign::load(root.join("data/campaign/sokoban.json")).expect("frozen campaign");
    let solution_source = fs::read_to_string(root.join("data/oracle/solutions.tsv"))
        .expect("complete Oracle solutions");
    let solutions = solution_source
        .lines()
        .map(|line| line.split_once('\t').expect("level and LURD directions"))
        .collect::<BTreeMap<_, _>>();
    assert_eq!(solutions.len(), campaign.max_score());

    let mut session = Session::new(&campaign);
    for tier in &campaign.tiers {
        for level in &tier.levels {
            session.select(&level.id).expect("level is unlocked");
            let directions = solutions
                .get(level.id.as_str())
                .unwrap_or_else(|| panic!("missing solution for {}", level.id))
                .chars()
                .map(|direction| Direction::parse(&direction.to_string()).expect("LURD direction"))
                .collect::<Vec<_>>();
            for chunk in directions.chunks(64) {
                session.move_many(chunk).expect("valid Oracle moves");
            }
            assert!(
                session.snapshot().level.is_some_and(|level| level.solved),
                "solution did not complete {}",
                level.id
            );
        }
    }

    let snapshot = session.snapshot();
    assert_eq!(snapshot.campaign.score, 305);
    assert_eq!(snapshot.campaign.max_score, 305);
    assert!(snapshot.campaign.complete);
    assert!(snapshot.tiers.iter().all(|tier| tier.status == "complete"));
}
