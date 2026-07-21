use std::collections::HashSet;
use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

pub const CAMPAIGN_SCHEMA: &str = "emergency-operator-campaign-v1";

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    Police,
    Fire,
    Medical,
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
pub struct RequirementDefinition {
    pub role: Role,
    pub work_ms: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct IncidentDefinition {
    pub id: String,
    pub title: String,
    pub location: Point,
    pub health_milli: i64,
    pub health_decay_milli_per_minute: i64,
    pub base_score: i64,
    pub requirements: Vec<RequirementDefinition>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DialogueChoice {
    pub id: String,
    pub text: String,
    #[serde(default)]
    pub next: Option<String>,
    #[serde(default)]
    pub reveal_incident: bool,
    #[serde(default)]
    pub health_decay_delta_milli_per_minute: i64,
    #[serde(default)]
    pub score_delta: i64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DialogueStage {
    pub id: String,
    pub caller: String,
    pub choices: Vec<DialogueChoice>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CallDefinition {
    pub id: String,
    pub caller: String,
    pub arrival_ms: u64,
    pub answer_window_ms: u64,
    pub conversation_window_ms: u64,
    pub initial_stage: String,
    pub stages: Vec<DialogueStage>,
    #[serde(default)]
    pub incident: Option<IncidentDefinition>,
}

impl CallDefinition {
    pub fn stage(&self, id: &str) -> Option<&DialogueStage> {
        self.stages.iter().find(|stage| stage.id == id)
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ShiftDefinition {
    pub id: String,
    pub title: String,
    pub duration_ms: u64,
    pub units: Vec<UnitDefinition>,
    pub calls: Vec<CallDefinition>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Campaign {
    pub schema: String,
    pub id: String,
    pub title: String,
    pub max_score: i64,
    pub shift: ShiftDefinition,
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
        for call in &self.shift.calls {
            if !call_ids.insert(&call.id) {
                return Err(format!("duplicate call id {:?}", call.id));
            }
            if call.arrival_ms >= self.shift.duration_ms
                || call.answer_window_ms == 0
                || call.conversation_window_ms == 0
            {
                return Err(format!("call {:?} has invalid timing", call.id));
            }
            let stage_ids = call
                .stages
                .iter()
                .map(|stage| stage.id.as_str())
                .collect::<HashSet<_>>();
            if stage_ids.len() != call.stages.len()
                || !stage_ids.contains(call.initial_stage.as_str())
            {
                return Err(format!("call {:?} has invalid dialogue stages", call.id));
            }
            for stage in &call.stages {
                if stage.choices.is_empty() {
                    return Err(format!(
                        "call {:?} stage {:?} has no choices",
                        call.id, stage.id
                    ));
                }
                let mut choices = HashSet::new();
                for choice in &stage.choices {
                    if !choices.insert(&choice.id) {
                        return Err(format!(
                            "call {:?} stage {:?} has duplicate choice {:?}",
                            call.id, stage.id, choice.id
                        ));
                    }
                    if choice
                        .next
                        .as_deref()
                        .is_some_and(|next| !stage_ids.contains(next))
                    {
                        return Err(format!(
                            "call {:?} choice {:?} points to an unknown stage",
                            call.id, choice.id
                        ));
                    }
                }
            }
            if let Some(incident) = &call.incident {
                if !incident_ids.insert(&incident.id) {
                    return Err(format!("duplicate incident id {:?}", incident.id));
                }
                if incident.health_milli <= 0 || incident.health_decay_milli_per_minute < 0 {
                    return Err(format!("incident {:?} has invalid health", incident.id));
                }
                if incident.requirements.is_empty() {
                    return Err(format!("incident {:?} has no requirements", incident.id));
                }
                let mut requirement_roles = HashSet::new();
                for requirement in &incident.requirements {
                    if requirement.work_ms == 0 || !roles.contains(&requirement.role) {
                        return Err(format!(
                            "incident {:?} has an unsupported requirement",
                            incident.id
                        ));
                    }
                    if !requirement_roles.insert(requirement.role) {
                        return Err(format!(
                            "incident {:?} has duplicate requirement role {:?}",
                            incident.id, requirement.role
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
}
