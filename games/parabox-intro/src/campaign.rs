use std::sync::LazyLock;

pub const CAMPAIGN_ID: &str = "parabox-complete-364-v11";
pub const STATE_HEADER: &str = "parabox-state-v4";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PuzzleKind {
    Core,
    Challenge,
    Side,
}

impl PuzzleKind {
    pub fn parse(value: &str) -> Self {
        match value {
            "core" => Self::Core,
            "challenge" => Self::Challenge,
            "side" => Self::Side,
            _ => panic!("unknown puzzle kind: {value}"),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Core => "core",
            Self::Challenge => "challenge",
            Self::Side => "side",
        }
    }
}

#[derive(Clone, Copy)]
pub struct Entry {
    pub reference: &'static str,
    pub title: &'static str,
    pub file: &'static str,
    pub area: &'static str,
    pub kind: PuzzleKind,
    pub predecessor: Option<&'static str>,
    pub immediate: bool,
}

#[derive(Clone, Copy)]
pub enum AreaAccess {
    Start,
    Gate(&'static str),
    Nexus(&'static str),
}

#[derive(Clone, Copy)]
pub struct Area {
    pub id: &'static str,
    pub name: &'static str,
    pub access: AreaAccess,
    pub required_adjustment: i32,
    pub lookahead: usize,
    pub has_gate: bool,
}

pub static LEVELS: LazyLock<Vec<Entry>> = LazyLock::new(|| {
    include_str!("../data/campaign/index.tsv")
        .lines()
        .map(|line| {
            let mut fields = line.split('\t');
            let reference = fields.next().expect("campaign entry has a reference");
            let title = fields.next().expect("campaign entry has a title");
            let file = fields.next().expect("campaign entry has a filename");
            let area = fields.next().expect("campaign entry has an area");
            let kind = PuzzleKind::parse(fields.next().expect("campaign entry has a kind"));
            let predecessor = match fields.next().expect("campaign entry has a predecessor") {
                "-" => None,
                value => Some(value),
            };
            let immediate = match fields.next().expect("campaign entry has an immediate flag") {
                "0" => false,
                "1" => true,
                value => panic!("invalid immediate flag: {value}"),
            };
            assert!(
                fields.next().is_none(),
                "campaign entry has unexpected fields"
            );
            Entry {
                reference,
                title,
                file,
                area,
                kind,
                predecessor,
                immediate,
            }
        })
        .collect()
});

pub static AREAS: LazyLock<Vec<Area>> = LazyLock::new(|| {
    include_str!("../data/campaign/areas.tsv")
        .lines()
        .map(|line| {
            let mut fields = line.split('\t');
            let id = fields.next().expect("area has an id");
            let name = fields.next().expect("area has a name");
            let access = match fields.next().expect("area has an access kind") {
                "start" => {
                    assert_eq!(fields.next(), Some("-"));
                    AreaAccess::Start
                }
                "gate" => AreaAccess::Gate(fields.next().expect("gate has a source area")),
                "nexus" => AreaAccess::Nexus(fields.next().expect("nexus has a source level")),
                value => panic!("invalid area access kind: {value}"),
            };
            let required_adjustment = fields
                .next()
                .expect("area has a required adjustment")
                .parse()
                .expect("area required adjustment is an integer");
            let lookahead = fields
                .next()
                .expect("area has a lookahead")
                .parse()
                .expect("area lookahead is an integer");
            let has_gate = match fields.next().expect("area has a gate flag") {
                "0" => false,
                "1" => true,
                value => panic!("invalid area gate flag: {value}"),
            };
            assert!(fields.next().is_none(), "area has unexpected fields");
            Area {
                id,
                name,
                access,
                required_adjustment,
                lookahead,
                has_gate,
            }
        })
        .collect()
});

pub fn level_index(reference: &str) -> Option<usize> {
    LEVELS.iter().position(|entry| entry.reference == reference)
}

pub fn area(id: &str) -> &'static Area {
    AREAS
        .iter()
        .find(|area| area.id == id)
        .unwrap_or_else(|| panic!("unknown area: {id}"))
}

pub fn gate_open(area_id: &str, solved: &[bool]) -> bool {
    let area = area(area_id);
    let solved_count = LEVELS
        .iter()
        .zip(solved)
        .filter(|(entry, won)| entry.area == area_id && **won)
        .count() as i32;
    let core_count = LEVELS
        .iter()
        .filter(|entry| entry.area == area_id && entry.kind == PuzzleKind::Core)
        .count() as i32;
    solved_count >= core_count + area.required_adjustment
}

pub fn area_available(area_id: &str, solved: &[bool]) -> bool {
    match area(area_id).access {
        AreaAccess::Start => true,
        AreaAccess::Gate(source) => area_available(source, solved) && gate_open(source, solved),
        AreaAccess::Nexus(reference) => level_index(reference).is_some_and(|index| solved[index]),
    }
}

pub fn level_available(index: usize, solved: &[bool]) -> bool {
    if solved[index] {
        return true;
    }
    let entry = LEVELS[index];
    if entry
        .predecessor
        .and_then(level_index)
        .is_some_and(|predecessor| solved[predecessor])
    {
        return true;
    }
    if !area_available(entry.area, solved) {
        return false;
    }

    match entry.kind {
        PuzzleKind::Challenge => entry.predecessor.is_none(),
        PuzzleKind::Side => {
            entry.predecessor.is_none()
                && (!area(entry.area).has_gate || gate_open(entry.area, solved))
        }
        PuzzleKind::Core => {
            let mut missing = 0;
            let mut predecessor = entry.predecessor.and_then(level_index);
            while let Some(index) = predecessor {
                if !solved[index] {
                    missing += 1;
                }
                predecessor = LEVELS[index].predecessor.and_then(level_index);
            }
            missing <= area(entry.area).lookahead
        }
    }
}

pub fn immediate_successor(index: usize) -> Option<usize> {
    LEVELS
        .iter()
        .position(|entry| entry.immediate && entry.predecessor == Some(LEVELS[index].reference))
}

#[cfg(test)]
mod tests {
    use super::{LEVELS, PuzzleKind, level_available, level_index};

    #[test]
    fn complete_original_catalog_is_present() {
        assert_eq!(LEVELS.len(), 364);
        assert_eq!(
            LEVELS
                .iter()
                .filter(|entry| entry.kind != PuzzleKind::Core)
                .count(),
            193
        );
        assert_eq!(LEVELS.last().unwrap().reference, "w8");
    }

    #[test]
    fn original_intro_starts_with_only_a1_available() {
        let solved = vec![false; LEVELS.len()];
        let available: Vec<_> = LEVELS
            .iter()
            .enumerate()
            .filter(|(index, _)| level_available(*index, &solved))
            .map(|(_, entry)| entry.reference)
            .collect();
        assert_eq!(available, ["a1"]);
    }

    #[test]
    fn solving_intro_opens_enter_area() {
        let mut solved = vec![false; LEVELS.len()];
        for number in 1..=9 {
            solved[level_index(&format!("a{number}")).unwrap()] = true;
        }
        assert!(level_available(level_index("b1").unwrap(), &solved));
    }

    #[test]
    fn every_puzzle_is_reachable_through_the_original_unlock_graph() {
        let mut solved = vec![false; LEVELS.len()];
        loop {
            let before = solved.iter().filter(|&&won| won).count();
            for index in 0..LEVELS.len() {
                if !solved[index] && level_available(index, &solved) {
                    solved[index] = true;
                }
            }
            let after = solved.iter().filter(|&&won| won).count();
            if after == LEVELS.len() {
                break;
            }
            assert!(after > before, "campaign unlock graph deadlocked");
        }
    }
}
