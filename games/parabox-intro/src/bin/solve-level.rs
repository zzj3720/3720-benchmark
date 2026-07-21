use std::collections::{HashSet, VecDeque};
use std::env;

use parabox_terminal::engine::{Direction, Game, load_level};

const DIRECTIONS: [Direction; 4] = [
    Direction::Up,
    Direction::Down,
    Direction::Left,
    Direction::Right,
];

fn solve(start: Game) -> Result<(Vec<Direction>, usize), String> {
    let mut seen = HashSet::from([start.state_key()]);
    let mut parents = vec![None];
    let mut pending = VecDeque::from([(start, 0)]);

    while let Some((game, node)) = pending.pop_front() {
        for direction in DIRECTIONS {
            let mut next = game.clone();
            if !next.move_player(direction) || !seen.insert(next.state_key()) {
                continue;
            }
            let next_node = parents.len();
            parents.push(Some((node, direction)));
            if next.won() {
                let mut history = Vec::new();
                let mut cursor = next_node;
                while let Some((parent, step)) = parents[cursor] {
                    history.push(step);
                    cursor = parent;
                }
                history.reverse();
                return Ok((history, seen.len()));
            }
            pending.push_back((next, next_node));
        }
    }
    Err("level has no solution under the implemented rules".to_string())
}

fn parse_actions(values: &[String]) -> Result<Vec<Direction>, String> {
    let mut actions = Vec::new();
    for value in values {
        if value.len() > 1 && value.chars().all(|ch| "UDLRudlr".contains(ch)) {
            for ch in value.chars() {
                actions.push(Direction::parse(&ch.to_string())?);
            }
        } else {
            actions.push(Direction::parse(value)?);
        }
    }
    Ok(actions)
}

fn run() -> Result<(), String> {
    let mut args = env::args().skip(1);
    let path = args
        .next()
        .ok_or_else(|| "usage: solve-level LEVEL [DIRECTION...]".to_string())?;
    let supplied: Vec<_> = args.collect();
    if !supplied.is_empty() {
        let actions = parse_actions(&supplied)?;
        let mut game = load_level(path)?;
        for action in &actions {
            game.move_player(*action);
        }
        if !game.won() {
            eprintln!("{}", game.render_current()?);
            return Err(format!(
                "supplied history does not solve the level after {} actions",
                actions.len()
            ));
        }
        println!("solved in {} supplied actions", actions.len());
        return Ok(());
    }

    let (history, visited) = solve(load_level(path)?)?;
    println!(
        "{}",
        history
            .iter()
            .map(|direction| direction.as_str())
            .collect::<Vec<_>>()
            .join(" ")
    );
    eprintln!("moves: {}; visited states: {visited}", history.len());
    Ok(())
}

fn main() {
    if let Err(error) = run() {
        eprintln!("solve-level: {error}");
        std::process::exit(1);
    }
}
