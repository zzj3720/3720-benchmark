use std::env;
use std::path::PathBuf;

use sausage_terminal::{Campaign, data_root};

fn main() {
    let mut args = env::args_os().skip(1);
    let path = args
        .next()
        .map(PathBuf::from)
        .unwrap_or_else(|| data_root().join("campaign").join("merged_binary.gz"));
    let campaign = Campaign::load_gzip(&path).unwrap_or_else(|error| {
        eprintln!("sausage-inspect: {error}");
        std::process::exit(1);
    });
    println!(
        "islands={} temples={} puzzles={} masks={} projections={}",
        campaign.island_names.len(),
        campaign.temples.len(),
        campaign.puzzle_ids().len(),
        campaign.island_masks.len(),
        campaign.projection_compatibilities.len()
    );
    if let Some(level) = args.next().and_then(|value| value.into_string().ok()) {
        let state = campaign
            .island_state(&level)
            .unwrap_or_else(|error| panic!("{level}: {error}"));
        println!("{}", state.display_name);
        for entity in state.entities {
            println!("{entity:?}");
        }
        return;
    }
    for (index, id) in campaign.puzzle_ids().iter().enumerate() {
        let state = campaign
            .island_state(id)
            .unwrap_or_else(|error| panic!("{id}: {error}"));
        println!("{:>2}\t{}\t{}", index + 1, id, state.display_name);
    }
}
