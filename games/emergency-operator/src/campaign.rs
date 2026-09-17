use std::collections::HashSet;
use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

pub const CAMPAIGN_SCHEMA: &str = "emergency-operator-campaign-v2";
const DEFAULT_CHANCE_WEIGHT: u32 = 330;

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    Police,
    Fire,
    Medical,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EventKind {
    #[default]
    Phone,
    Report,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SceneKind {
    Criminal,
    Suspect,
    Injured,
    Dead,
    Fire,
    Tech,
    Work,
    Timer,
    Witness,
    Passerby,
    Deco,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Point {
    pub x: i32,
    pub y: i32,
}

impl Point {
    pub fn distance(self, other: Self) -> u64 {
        self.x.abs_diff(other.x) as u64 + self.y.abs_diff(other.y) as u64
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct UnitDefinition {
    pub id: String,
    pub label: String,
    pub role: Role,
    pub base: Point,
    pub speed_cells_per_minute: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct IncidentDefinition {
    pub id: String,
    pub title: String,
    pub location: Point,
    pub base_score: i64,
    pub elements: Vec<SceneElementDefinition>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SceneElementDefinition {
    pub id: String,
    pub label: String,
    pub kind: SceneKind,
    #[serde(default = "default_true")]
    pub active: bool,
    #[serde(default)]
    pub role: Option<Role>,
    #[serde(default)]
    pub health_milli: Option<i64>,
    #[serde(default)]
    pub health_decay_milli_per_minute: i64,
    #[serde(default)]
    pub work_ms: u64,
    #[serde(default)]
    pub work_growth_ms_per_minute: i64,
    #[serde(default)]
    pub blocked_by: Option<String>,
    #[serde(default)]
    pub weapon: Option<String>,
    #[serde(default)]
    pub fight_risk_milli: i64,
    #[serde(default)]
    pub prison_chance_milli: i64,
    #[serde(default)]
    pub bill: Option<i64>,
    #[serde(default)]
    pub timer_ms: Option<u64>,
    #[serde(default)]
    pub actions: Vec<String>,
    #[serde(default)]
    pub aar: Vec<String>,
}

const fn default_true() -> bool {
    true
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DialogueNode {
    pub id: String,
    pub operator: bool,
    pub text: String,
    #[serde(default)]
    pub answers: Vec<String>,
    #[serde(default = "default_chance_weight")]
    pub chance_weight: u32,
    #[serde(default)]
    pub actions: Vec<String>,
    #[serde(default)]
    pub aar: Vec<String>,
}

const fn default_chance_weight() -> u32 {
    DEFAULT_CHANCE_WEIGHT
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CallDefinition {
    pub id: String,
    pub caller: String,
    #[serde(default)]
    pub kind: EventKind,
    #[serde(default)]
    pub duty_id: Option<String>,
    pub arrival_ms: u64,
    pub answer_window_ms: u64,
    pub conversation_window_ms: u64,
    pub initial_stage: String,
    #[serde(default)]
    pub nodes: Vec<DialogueNode>,
    #[serde(default)]
    pub disabled_nodes: Vec<String>,
    #[serde(default)]
    pub incident: Option<IncidentDefinition>,
}

impl CallDefinition {
    pub fn node(&self, id: &str) -> Option<&DialogueNode> {
        self.nodes.iter().find(|node| node.id == id)
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DutyDefinition {
    pub id: String,
    pub chapter: u32,
    pub number: u32,
    pub city: String,
    pub map_id: String,
    pub start_ms: u64,
    pub end_ms: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ShiftDefinition {
    pub id: String,
    pub title: String,
    pub duration_ms: u64,
    #[serde(default)]
    pub duties: Vec<DutyDefinition>,
    pub units: Vec<UnitDefinition>,
    pub calls: Vec<CallDefinition>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Campaign {
    pub schema: String,
    pub id: String,
    pub title: String,
    #[serde(default = "default_seed")]
    pub seed: u64,
    pub max_score: i64,
    pub shift: ShiftDefinition,
}

const fn default_seed() -> u64 {
    0x9110_3720
}

impl Campaign {
    pub fn load(path: impl AsRef<Path>) -> Result<Self, String> {
        let path = path.as_ref();
        let bytes = fs::read(path)
            .map_err(|error| format!("could not read {}: {error}", path.display()))?;
        let campaign: Self = serde_json::from_slice(&bytes)
            .map_err(|error| format!("invalid {}: {error}", path.display()))?;
        campaign.validate()?;
        Ok(campaign)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != CAMPAIGN_SCHEMA {
            return Err(format!("unknown campaign schema {:?}", self.schema));
        }
        if self.id.is_empty() || self.shift.id.is_empty() || self.shift.duration_ms == 0 {
            return Err("campaign, shift, and positive duration are required".to_owned());
        }
        if self.max_score <= 0 {
            return Err("campaign max_score must be positive".to_owned());
        }

        let mut unit_ids = HashSet::new();
        let mut roles = HashSet::new();
        for unit in &self.shift.units {
            if !unit_ids.insert(&unit.id) {
                return Err(format!("duplicate unit id {:?}", unit.id));
            }
            if unit.speed_cells_per_minute == 0 {
                return Err(format!("unit {:?} has zero speed", unit.id));
            }
            roles.insert(unit.role);
        }

        let mut call_ids = HashSet::new();
        let mut incident_ids = HashSet::new();
        let mut duty_ids = HashSet::new();
        let mut previous_duty_end = 0;
        for duty in &self.shift.duties {
            if !duty_ids.insert(duty.id.as_str())
                || duty.id.is_empty()
                || duty.city.is_empty()
                || duty.map_id.is_empty()
                || duty.start_ms != previous_duty_end
                || duty.start_ms >= duty.end_ms
                || duty.end_ms > self.shift.duration_ms
            {
                return Err(format!("duty {:?} has invalid boundaries", duty.id));
            }
            previous_duty_end = duty.end_ms;
        }
        if !self.shift.duties.is_empty() && previous_duty_end != self.shift.duration_ms {
            return Err("duties do not cover the full shift".to_owned());
        }
        for call in &self.shift.calls {
            if !call_ids.insert(&call.id) {
                return Err(format!("duplicate call id {:?}", call.id));
            }
            if call.arrival_ms >= self.shift.duration_ms {
                return Err(format!("call {:?} has invalid timing", call.id));
            }
            if let Some(duty_id) = &call.duty_id {
                let duty = self
                    .shift
                    .duties
                    .iter()
                    .find(|duty| &duty.id == duty_id)
                    .ok_or_else(|| format!("call {:?} has an unknown duty", call.id))?;
                if call.arrival_ms < duty.start_ms || call.arrival_ms >= duty.end_ms {
                    return Err(format!("call {:?} falls outside its duty", call.id));
                }
            }
            if call.kind == EventKind::Report {
                if call.incident.is_none() || !call.nodes.is_empty() {
                    return Err(format!("report {:?} has invalid content", call.id));
                }
            } else if call.answer_window_ms == 0 || call.conversation_window_ms == 0 {
                return Err(format!("call {:?} has invalid timing", call.id));
            } else {
                let node_ids = call
                    .nodes
                    .iter()
                    .map(|node| node.id.as_str())
                    .collect::<HashSet<_>>();
                if node_ids.len() != call.nodes.len()
                    || !node_ids.contains(call.initial_stage.as_str())
                {
                    return Err(format!("call {:?} has an invalid dialogue graph", call.id));
                }
                for node in &call.nodes {
                    if node.chance_weight == 0 {
                        return Err(format!(
                            "call {:?} node {:?} has zero chance weight",
                            call.id, node.id
                        ));
                    }
                    for answer in &node.answers {
                        if answer != "back" && !node_ids.contains(answer.as_str()) {
                            return Err(format!(
                                "call {:?} node {:?} points to unknown answer {:?}",
                                call.id, node.id, answer
                            ));
                        }
                    }
                }
                for disabled in &call.disabled_nodes {
                    if !node_ids.contains(disabled.as_str()) {
                        return Err(format!(
                            "call {:?} disables unknown node {:?}",
                            call.id, disabled
                        ));
                    }
                }
            }
            if let Some(incident) = &call.incident {
                if !incident_ids.insert(&incident.id) {
                    return Err(format!("duplicate incident id {:?}", incident.id));
                }
                if incident.elements.is_empty() {
                    return Err(format!("incident {:?} has no scene elements", incident.id));
                }
                let element_ids = incident
                    .elements
                    .iter()
                    .map(|element| element.id.as_str())
                    .collect::<HashSet<_>>();
                if element_ids.len() != incident.elements.len() {
                    return Err(format!("incident {:?} has duplicate elements", incident.id));
                }
                for element in &incident.elements {
                    if element.id.is_empty()
                        || element.label.is_empty()
                        || (element.kind != SceneKind::Dead
                            && element.health_milli.is_some_and(|health| health <= 0))
                        || element.health_decay_milli_per_minute < 0
                        || element.work_growth_ms_per_minute < 0
                        || element
                            .blocked_by
                            .as_deref()
                            .is_some_and(|blocker| !element_ids.contains(blocker))
                        || (element.work_ms > 0 && element.role.is_none())
                        || (element.kind == SceneKind::Timer
                            && element.timer_ms.is_none_or(|time| time == 0))
                        || (element.kind != SceneKind::Timer && element.timer_ms.is_some())
                        || element.role.is_some_and(|role| !roles.contains(&role))
                    {
                        return Err(format!(
                            "incident {:?} has invalid scene element {:?}",
                            incident.id, element.id
                        ));
                    }
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pilot_campaign_is_valid() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let campaign =
            Campaign::load(root.join("data/campaign/pilot.json")).expect("pilot campaign");
        assert_eq!(campaign.shift.calls.len(), 3);
        assert_eq!(campaign.shift.units.len(), 5);
    }

    #[test]
    fn imported_base_career_is_valid() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let campaign = Campaign::load(root.join("data/campaign/911-career.json"))
            .expect("compiled 911 Operator career");
        assert_eq!(campaign.shift.duties.len(), 14);
        assert_eq!(campaign.shift.calls.len(), 102);
        assert_eq!(campaign.shift.duration_ms, 30 * 60_000);
        assert_eq!(campaign.max_score, 34_405);
        assert_eq!(
            campaign
                .shift
                .calls
                .iter()
                .map(|call| call.nodes.len())
                .sum::<usize>(),
            2_722
        );
    }
}
