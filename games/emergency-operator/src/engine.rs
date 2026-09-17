use std::collections::{BTreeMap, HashSet};

use serde::Serialize;

use crate::campaign::{
    CallDefinition, Campaign, EventKind, IncidentDefinition, Point, Role, SceneKind,
};

pub const STATE_SCHEMA: &str = "emergency-operator-state-v1";

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
enum CallPhase {
    Scheduled,
    Ringing,
    Active,
    Completed,
    Missed,
    Dropped,
}

#[derive(Clone, Debug)]
struct CallRuntime {
    phase: CallPhase,
    current_stage: Option<String>,
    answer_deadline_ms: u64,
    conversation_deadline_ms: Option<u64>,
    disabled_nodes: HashSet<String>,
    history: Vec<String>,
    opinion_milli: i64,
    ignore_opinion_milli: Option<i64>,
    aar: Vec<String>,
    facts: BTreeMap<String, String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
enum IncidentPhase {
    Hidden,
    Reported,
    Resolved,
    Lost,
}

#[derive(Clone, Debug)]
struct SceneElementRuntime {
    id: String,
    label: String,
    kind: SceneKind,
    active: bool,
    role: Option<Role>,
    health_milli: Option<i64>,
    health_decay_milli_per_minute: i64,
    health_remainder: i64,
    remaining_work_ms: u64,
    total_work_ms: u64,
    work_growth_ms_per_minute: i64,
    work_remainder: i64,
    blocked_by: Option<String>,
    weapon: Option<String>,
    fight_risk_milli: i64,
    prison_chance_milli: i64,
    bill: Option<i64>,
    remaining_timer_ms: Option<u64>,
    actions: Vec<String>,
    aar: Vec<String>,
    completed: bool,
}

#[derive(Clone, Debug)]
struct IncidentRuntime {
    call_index: usize,
    phase: IncidentPhase,
    elements: Vec<SceneElementRuntime>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum UnitPhase {
    Idle,
    EnRoute {
        incident_index: usize,
        arrival_ms: u64,
    },
    OnScene {
        incident_index: usize,
    },
    Returning {
        arrival_ms: u64,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
enum AlarmPhase {
    Pending,
    Due,
    Delivered,
    Cancelled,
}

#[derive(Clone, Debug)]
struct AlarmRuntime {
    id: String,
    note: String,
    due_ms: u64,
    phase: AlarmPhase,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct CampaignView {
    pub id: String,
    pub title: String,
    pub score: i64,
    pub max_score: i64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ShiftView {
    pub id: String,
    pub title: String,
    pub status: String,
    pub elapsed_ms: u64,
    pub duration_ms: u64,
    pub remaining_ms: u64,
    pub duty: Option<DutyView>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct DutyView {
    pub id: String,
    pub chapter: u32,
    pub number: u32,
    pub city: String,
    pub map_id: String,
    pub elapsed_ms: u64,
    pub remaining_ms: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ChoiceView {
    pub id: String,
    pub text: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct TranscriptView {
    pub speaker: &'static str,
    pub text: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct CallView {
    pub id: String,
    pub caller: String,
    pub status: String,
    pub answer_deadline_ms: Option<u64>,
    pub conversation_deadline_ms: Option<u64>,
    pub caller_text: Option<String>,
    pub choices: Vec<ChoiceView>,
    pub transcript: Vec<TranscriptView>,
    pub opinion_milli: i64,
    pub after_action_report: Vec<String>,
    pub facts: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct RequirementView {
    pub role: Role,
    pub remaining_work_ms: u64,
    pub total_work_ms: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct IncidentView {
    pub id: String,
    pub title: String,
    pub status: String,
    pub location: Point,
    pub health_milli: i64,
    pub health_decay_milli_per_minute: i64,
    pub requirements: Vec<RequirementView>,
    pub elements: Vec<SceneElementView>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct SceneElementView {
    pub id: String,
    pub label: String,
    pub kind: SceneKind,
    pub active: bool,
    pub role: Option<Role>,
    pub health_milli: Option<i64>,
    pub health_decay_milli_per_minute: i64,
    pub remaining_work_ms: u64,
    pub total_work_ms: u64,
    pub blocked_by: Option<String>,
    pub weapon: Option<String>,
    pub fight_risk_milli: i64,
    pub prison_chance_milli: i64,
    pub bill: Option<i64>,
    pub remaining_timer_ms: Option<u64>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct UnitView {
    pub id: String,
    pub label: String,
    pub role: Role,
    pub status: String,
    pub incident: Option<String>,
    pub eta_ms: Option<u64>,
    pub location: Point,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct AlarmView {
    pub id: String,
    pub note: String,
    pub due_ms: u64,
    pub status: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct Snapshot {
    pub schema: &'static str,
    pub campaign: CampaignView,
    pub shift: ShiftView,
    pub active_call: Option<String>,
    pub calls: Vec<CallView>,
    pub incidents: Vec<IncidentView>,
    pub units: Vec<UnitView>,
    pub alarms: Vec<AlarmView>,
    pub next_alarm_ms: Option<u64>,
    pub controls: Vec<&'static str>,
}

pub struct Session<'a> {
    campaign: &'a Campaign,
    started: bool,
    elapsed_ms: u64,
    score: i64,
    calls: Vec<CallRuntime>,
    incidents: Vec<IncidentRuntime>,
    units: Vec<UnitPhase>,
    alarms: Vec<AlarmRuntime>,
    rng_state: u64,
}

impl<'a> Session<'a> {
    pub fn new(campaign: &'a Campaign) -> Self {
        let calls = campaign
            .shift
            .calls
            .iter()
            .map(|call| CallRuntime {
                phase: CallPhase::Scheduled,
                current_stage: None,
                answer_deadline_ms: call.arrival_ms.saturating_add(call.answer_window_ms),
                conversation_deadline_ms: None,
                disabled_nodes: call.disabled_nodes.iter().cloned().collect(),
                history: Vec::new(),
                opinion_milli: 0,
                ignore_opinion_milli: None,
                aar: Vec::new(),
                facts: BTreeMap::new(),
            })
            .collect();
        let incidents = campaign
            .shift
            .calls
            .iter()
            .enumerate()
            .filter_map(|(call_index, call)| {
                call.incident.as_ref().map(|incident| IncidentRuntime {
                    call_index,
                    phase: IncidentPhase::Hidden,
                    elements: runtime_elements(incident),
                })
            })
            .collect();
        let units = vec![UnitPhase::Idle; campaign.shift.units.len()];
        Self {
            campaign,
            started: false,
            elapsed_ms: 0,
            score: 0,
            calls,
            incidents,
            units,
            alarms: Vec::new(),
            rng_state: campaign.seed.max(1),
        }
    }

    pub const fn started(&self) -> bool {
        self.started
    }

    pub const fn elapsed_ms(&self) -> u64 {
        self.elapsed_ms
    }

    pub const fn duration_ms(&self) -> u64 {
        self.campaign.shift.duration_ms
    }

    pub fn start(&mut self) -> Result<(), String> {
        if self.started {
            return Err("shift has already started".to_owned());
        }
        self.started = true;
        self.process_due();
        Ok(())
    }

    pub fn advance_to(&mut self, target_ms: u64) -> Result<(), String> {
        if !self.started {
            if target_ms == 0 {
                return Ok(());
            }
            return Err("cannot advance a shift before it starts".to_owned());
        }
        if target_ms < self.elapsed_ms {
            return Err(format!(
                "clock cannot move backwards from {} to {target_ms}",
                self.elapsed_ms
            ));
        }
        let target_ms = target_ms.min(self.duration_ms());
        self.process_due();
        while self.elapsed_ms < target_ms {
            let next = self.next_boundary(target_ms).max(self.elapsed_ms + 1);
            self.advance_continuous(next - self.elapsed_ms);
            self.elapsed_ms = next;
            self.process_due();
        }
        Ok(())
    }

    pub fn answer(&mut self, call_id: &str) -> Result<(), String> {
        self.require_running()?;
        if self
            .calls
            .iter()
            .any(|call| call.phase == CallPhase::Active)
        {
            return Err("another call is already active".to_owned());
        }
        let index = self.call_index(call_id)?;
        let definition = &self.campaign.shift.calls[index];
        let conversation_deadline_ms = self
            .elapsed_ms
            .saturating_add(definition.conversation_window_ms)
            .min(self.duration_ms());
        let call = &mut self.calls[index];
        if call.phase != CallPhase::Ringing {
            return Err(format!("call {call_id:?} is not ringing"));
        }
        call.phase = CallPhase::Active;
        call.conversation_deadline_ms = Some(conversation_deadline_ms);
        call.current_stage = None;
        call.history.clear();
        let initial = definition.initial_stage.clone();
        self.enter_graph_node(index, initial)?;
        Ok(())
    }

    pub fn say(&mut self, call_id: &str, choice_id: &str) -> Result<(), String> {
        self.require_running()?;
        let call_index = self.call_index(call_id)?;
        self.say_graph(call_index, choice_id)
    }

    fn say_graph(&mut self, call_index: usize, choice_id: &str) -> Result<(), String> {
        if self.calls[call_index].phase != CallPhase::Active {
            let id = &self.campaign.shift.calls[call_index].id;
            return Err(format!("call {id:?} is not active"));
        }
        let current = self.calls[call_index]
            .current_stage
            .clone()
            .ok_or_else(|| "active graph call has no current node".to_owned())?;
        let available = self.graph_answers(call_index, &current)?;
        if !available.iter().any(|answer| answer == choice_id) {
            return Err(format!("choice {choice_id:?} is not available"));
        }
        self.enter_graph_node(call_index, choice_id.to_owned())
    }

    fn enter_graph_node(&mut self, call_index: usize, mut node_id: String) -> Result<(), String> {
        let definition = &self.campaign.shift.calls[call_index];
        let limit = definition.nodes.len().saturating_mul(3).max(1);
        for _ in 0..limit {
            let node = self.campaign.shift.calls[call_index]
                .node(&node_id)
                .cloned()
                .ok_or_else(|| format!("unknown dialogue node {node_id:?}"))?;
            if self.calls[call_index].disabled_nodes.contains(&node.id) {
                return Err(format!("dialogue node {:?} is disabled", node.id));
            }
            if node.operator {
                self.calls[call_index]
                    .disabled_nodes
                    .insert(node.id.clone());
            }
            self.calls[call_index].history.push(node.id.clone());
            self.calls[call_index].current_stage = Some(node.id.clone());
            self.apply_graph_actions(call_index, &node.actions)?;
            self.calls[call_index].aar.extend(node.aar);
            if self.calls[call_index].phase != CallPhase::Active {
                return Ok(());
            }
            if !node.operator {
                if self.graph_answers(call_index, &node.id)?.is_empty() {
                    self.complete_call(call_index);
                }
                return Ok(());
            }

            let answers = node
                .answers
                .iter()
                .filter(|answer| {
                    answer.as_str() != "back"
                        && !self.calls[call_index]
                            .disabled_nodes
                            .contains(answer.as_str())
                })
                .filter_map(|answer| {
                    self.campaign.shift.calls[call_index]
                        .node(answer)
                        .map(|target| (target.id.clone(), target.chance_weight))
                })
                .collect::<Vec<_>>();
            let Some(next) = self.weighted_answer(&answers) else {
                self.complete_call(call_index);
                return Ok(());
            };
            node_id = next;
        }
        Err("dialogue graph exceeded its automatic traversal limit".to_owned())
    }

    fn graph_answers(&self, call_index: usize, node_id: &str) -> Result<Vec<String>, String> {
        self.graph_answers_before(call_index, node_id, self.calls[call_index].history.len())
    }

    fn graph_answers_before(
        &self,
        call_index: usize,
        node_id: &str,
        before: usize,
    ) -> Result<Vec<String>, String> {
        let definition = &self.campaign.shift.calls[call_index];
        let node = definition
            .node(node_id)
            .ok_or_else(|| format!("unknown dialogue node {node_id:?}"))?;
        let history = &self.calls[call_index].history;
        let current_position = history[..before.min(history.len())]
            .iter()
            .rposition(|visited| visited == node_id)
            .unwrap_or(before.min(history.len()));
        let mut answers = Vec::new();
        for answer in &node.answers {
            if answer == "back" {
                if let Some(previous_position) =
                    history[..current_position].iter().rposition(|visited| {
                        definition
                            .node(visited)
                            .is_some_and(|previous| !previous.operator)
                    })
                {
                    let previous_caller = &history[previous_position];
                    for nested in self.graph_answers_before(
                        call_index,
                        previous_caller,
                        previous_position + 1,
                    )? {
                        if !answers.contains(&nested) {
                            answers.push(nested);
                        }
                    }
                }
            } else if !self.calls[call_index]
                .disabled_nodes
                .contains(answer.as_str())
            {
                answers.push(answer.clone());
            }
        }
        Ok(answers)
    }

    fn weighted_answer(&mut self, answers: &[(String, u32)]) -> Option<String> {
        let total = answers
            .iter()
            .fold(0_u64, |sum, (_, weight)| sum.saturating_add(*weight as u64));
        if total == 0 {
            return None;
        }
        self.rng_state ^= self.rng_state << 13;
        self.rng_state ^= self.rng_state >> 7;
        self.rng_state ^= self.rng_state << 17;
        let pick = self.rng_state % total;
        let mut cursor = 0_u64;
        answers.iter().find_map(|(id, weight)| {
            cursor = cursor.saturating_add(*weight as u64);
            (pick < cursor).then(|| id.clone())
        })
    }

    fn apply_graph_actions(&mut self, call_index: usize, actions: &[String]) -> Result<(), String> {
        for raw in actions {
            for action in raw.split(';') {
                let action = action
                    .chars()
                    .filter(|character| !character.is_whitespace())
                    .collect::<String>()
                    .to_ascii_lowercase();
                if action.is_empty() {
                    continue;
                }
                if action == "actionhangup" {
                    self.complete_call(call_index);
                    continue;
                }
                if action == "actionsetlocation" {
                    self.reveal_incident(call_index);
                    continue;
                }
                if let Some((node, assignment)) = action.split_once("->") {
                    if let Some((field, value)) = assignment.split_once('=')
                        && matches!(field, "active" | "isactive" | "action")
                    {
                        let active = parse_bool(value)?;
                        if self.campaign.shift.calls[call_index].node(node).is_some() {
                            if active {
                                self.calls[call_index].disabled_nodes.remove(node);
                            } else {
                                self.calls[call_index]
                                    .disabled_nodes
                                    .insert(node.to_owned());
                            }
                        }
                    }
                    continue;
                }
                if let Some(assignment) = action.strip_prefix("opinioneffect") {
                    let old = self.calls[call_index].opinion_milli;
                    let next = apply_number(old, assignment, 1_000)?;
                    self.calls[call_index].opinion_milli = next;
                    self.score += (next - old) / 100;
                    continue;
                }
                if let Some(assignment) = action.strip_prefix("onignore") {
                    let old = self.calls[call_index].ignore_opinion_milli.unwrap_or(0);
                    self.calls[call_index].ignore_opinion_milli =
                        Some(apply_number(old, assignment, 1_000)?);
                    continue;
                }
                if let Some(assignment) = action.strip_prefix("score") {
                    self.score = apply_number(self.score, assignment, 1)?;
                    continue;
                }
                if let Some(assignment) = action.strip_prefix("addcash") {
                    self.score = apply_number(self.score, assignment, 1)?;
                    continue;
                }
                if let Some((element, property)) = action.split_once('.') {
                    self.apply_incident_property(call_index, element, property)?;
                    continue;
                }
                if let Some((element, kind)) = action.split_once('=')
                    && let Some(kind) = parse_scene_kind(kind)
                {
                    self.change_scene_kind(call_index, element, kind);
                    continue;
                }
                if let Some((key, value)) = action.split_once('=') {
                    self.calls[call_index]
                        .facts
                        .insert(key.to_owned(), value.to_owned());
                }
            }
        }
        Ok(())
    }

    fn apply_incident_property(
        &mut self,
        call_index: usize,
        element_id: &str,
        property: &str,
    ) -> Result<(), String> {
        let Some(incident_index) = self.incident_for_call(call_index) else {
            return Ok(());
        };
        let field_end = property.find(['=', '+', '-']).unwrap_or(property.len());
        let field = &property[..field_end];
        let elements = &mut self.incidents[incident_index].elements;
        let exact = elements.iter().position(|element| element.id == element_id);
        let health_element = || {
            exact.or_else(|| {
                elements
                    .iter()
                    .position(|element| element.health_milli.is_some())
            })
        };
        match field {
            "hp" => {
                if let Some(index) = health_element() {
                    let old = elements[index].health_milli.unwrap_or(100_000);
                    elements[index].health_milli =
                        Some(apply_number(old, &property[field_end..], 1_000)?.clamp(0, 100_000));
                }
            }
            "hpchange" | "chchange" => {
                if let Some(index) = health_element() {
                    let current = -elements[index].health_decay_milli_per_minute;
                    let change = apply_number(current, &property[field_end..], 60_000)?;
                    elements[index].health_decay_milli_per_minute = (-change).max(0);
                }
            }
            "healthdecay" => {
                if let Some(index) = health_element() {
                    let current = elements[index].health_decay_milli_per_minute;
                    elements[index].health_decay_milli_per_minute =
                        apply_number(current, &property[field_end..], 1)?.max(0);
                }
            }
            "isactive" => {
                if let Some(index) = exact {
                    elements[index].active = parse_bool(&property[field_end + 1..])?;
                    if elements[index].active
                        && self.incidents[incident_index].phase == IncidentPhase::Resolved
                    {
                        self.incidents[incident_index].phase = IncidentPhase::Reported;
                    }
                }
            }
            "work" => {
                if let Some(index) = exact {
                    let next = apply_number(
                        elements[index].remaining_work_ms as i64,
                        &property[field_end..],
                        1_000,
                    )?
                    .max(0) as u64;
                    elements[index].remaining_work_ms = next;
                    elements[index].total_work_ms = elements[index].total_work_ms.max(next);
                    if next > 0 {
                        elements[index].completed = false;
                    }
                }
            }
            "workchange" => {
                if let Some(index) = exact {
                    elements[index].work_growth_ms_per_minute = apply_number(
                        elements[index].work_growth_ms_per_minute,
                        &property[field_end..],
                        60_000,
                    )?
                    .max(0);
                }
            }
            "time" => {
                if let Some(index) = exact
                    && let Some(current) = elements[index].remaining_timer_ms
                {
                    let next =
                        apply_number(current as i64, &property[field_end..], 1_000)?.max(0) as u64;
                    elements[index].remaining_timer_ms = Some(next);
                    if next > 0 {
                        elements[index].completed = false;
                    }
                }
            }
            "weapon" => {
                if let Some(index) = exact {
                    elements[index].weapon = property[field_end + 1..]
                        .split_once('=')
                        .map(|(_, value)| value.to_owned())
                        .or_else(|| Some(property[field_end + 1..].to_owned()));
                }
            }
            "fightrisk" => {
                if let Some(index) = exact {
                    elements[index].fight_risk_milli = apply_number(
                        elements[index].fight_risk_milli,
                        &property[field_end..],
                        1_000,
                    )?;
                }
            }
            "prisonchance" => {
                if let Some(index) = exact {
                    elements[index].prison_chance_milli = apply_number(
                        elements[index].prison_chance_milli,
                        &property[field_end..],
                        1_000,
                    )?;
                }
            }
            "bill" => {
                if let Some(index) = exact {
                    let old = elements[index].bill.unwrap_or(0);
                    elements[index].bill =
                        Some(apply_number(old, &property[field_end..], 1)?.max(0));
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn change_scene_kind(&mut self, call_index: usize, element_id: &str, kind: SceneKind) {
        let Some(incident_index) = self.incident_for_call(call_index) else {
            return;
        };
        if let Some(element) = self.incidents[incident_index]
            .elements
            .iter_mut()
            .find(|element| element.id == element_id)
        {
            element.kind = kind;
            element.role = role_for_scene(kind);
        }
    }

    fn reveal_incident(&mut self, call_index: usize) {
        if let Some(incident_index) = self.incident_for_call(call_index) {
            let incident = &mut self.incidents[incident_index];
            if incident.phase == IncidentPhase::Hidden {
                incident.phase = IncidentPhase::Reported;
            }
        }
    }

    fn complete_call(&mut self, call_index: usize) {
        let call = &mut self.calls[call_index];
        call.phase = CallPhase::Completed;
        call.current_stage = None;
        call.conversation_deadline_ms = None;
    }

    pub fn dispatch(&mut self, unit_id: &str, incident_id: &str) -> Result<u64, String> {
        self.require_running()?;
        let unit_index = self.unit_index(unit_id)?;
        let incident_index = self.incident_index(incident_id)?;
        if self.units[unit_index] != UnitPhase::Idle {
            return Err(format!("unit {unit_id:?} is not idle"));
        }
        if self.incidents[incident_index].phase != IncidentPhase::Reported {
            return Err(format!("incident {incident_id:?} is not dispatchable"));
        }
        let travel_ms = self.travel_ms(unit_index, incident_index);
        let arrival_ms = self.elapsed_ms.saturating_add(travel_ms);
        self.units[unit_index] = UnitPhase::EnRoute {
            incident_index,
            arrival_ms,
        };
        Ok(arrival_ms)
    }

    pub fn recall(&mut self, unit_id: &str) -> Result<u64, String> {
        self.require_running()?;
        let unit_index = self.unit_index(unit_id)?;
        let incident_index = match self.units[unit_index] {
            UnitPhase::EnRoute { incident_index, .. } | UnitPhase::OnScene { incident_index } => {
                incident_index
            }
            _ => return Err(format!("unit {unit_id:?} cannot be recalled")),
        };
        let arrival_ms = self
            .elapsed_ms
            .saturating_add(self.travel_ms(unit_index, incident_index));
        self.units[unit_index] = UnitPhase::Returning { arrival_ms };
        Ok(arrival_ms)
    }

    pub fn set_alarm(&mut self, id: &str, after_ms: u64, note: &str) -> Result<u64, String> {
        self.require_running()?;
        if id.is_empty() || id.len() > 48 {
            return Err("alarm id must contain 1 to 48 characters".to_owned());
        }
        if note.len() > 160 {
            return Err("alarm note must contain at most 160 characters".to_owned());
        }
        if after_ms < 1_000 {
            return Err("alarm must be at least 1000 ms in the future".to_owned());
        }
        if self.alarms.iter().any(|alarm| {
            alarm.id == id && matches!(alarm.phase, AlarmPhase::Pending | AlarmPhase::Due)
        }) {
            return Err(format!("alarm {id:?} is already pending"));
        }
        let due_ms = self.elapsed_ms.saturating_add(after_ms);
        if due_ms > self.duration_ms() {
            return Err("alarm would fire after the shift ends".to_owned());
        }
        self.alarms.push(AlarmRuntime {
            id: id.to_owned(),
            note: note.to_owned(),
            due_ms,
            phase: AlarmPhase::Pending,
        });
        Ok(due_ms)
    }

    pub fn cancel_alarm(&mut self, id: &str) -> Result<(), String> {
        self.require_running()?;
        let alarm = self
            .alarms
            .iter_mut()
            .rev()
            .find(|alarm| alarm.id == id && alarm.phase == AlarmPhase::Pending)
            .ok_or_else(|| format!("alarm {id:?} is not pending"))?;
        alarm.phase = AlarmPhase::Cancelled;
        Ok(())
    }

    pub fn deliver_due_alarms(&mut self) -> Vec<AlarmView> {
        let mut delivered = Vec::new();
        for alarm in &mut self.alarms {
            if alarm.phase == AlarmPhase::Due {
                alarm.phase = AlarmPhase::Delivered;
                delivered.push(alarm_view(alarm));
            }
        }
        delivered
    }

    pub fn next_pending_alarm_ms(&self) -> Option<u64> {
        self.alarms
            .iter()
            .filter(|alarm| alarm.phase == AlarmPhase::Pending)
            .map(|alarm| alarm.due_ms)
            .min()
    }

    pub fn snapshot(&self) -> Snapshot {
        let current_duty_id = self
            .campaign
            .shift
            .duties
            .iter()
            .find(|duty| self.elapsed_ms >= duty.start_ms && self.elapsed_ms < duty.end_ms)
            .map(|duty| duty.id.as_str());
        let calls = self
            .campaign
            .shift
            .calls
            .iter()
            .zip(&self.calls)
            .enumerate()
            .filter(|(_, (definition, runtime))| {
                runtime.phase != CallPhase::Scheduled
                    && (definition.duty_id.is_none()
                        || definition.duty_id.as_deref() == current_duty_id
                        || matches!(runtime.phase, CallPhase::Ringing | CallPhase::Active))
            })
            .map(|(index, (definition, runtime))| self.call_view(index, definition, runtime))
            .collect();
        let incidents = self
            .incidents
            .iter()
            .enumerate()
            .filter(|(_, runtime)| {
                let definition = &self.campaign.shift.calls[runtime.call_index];
                runtime.phase != IncidentPhase::Hidden
                    && (definition.duty_id.is_none()
                        || definition.duty_id.as_deref() == current_duty_id
                        || runtime.phase == IncidentPhase::Reported)
            })
            .map(|(_, runtime)| self.incident_view(runtime))
            .collect();
        let units = self
            .campaign
            .shift
            .units
            .iter()
            .zip(&self.units)
            .map(|(definition, phase)| self.unit_view(definition.id.as_str(), phase))
            .collect();
        let status = if !self.started {
            "not_started"
        } else if self.elapsed_ms >= self.duration_ms() {
            "complete"
        } else {
            "running"
        };
        Snapshot {
            schema: STATE_SCHEMA,
            campaign: CampaignView {
                id: self.campaign.id.clone(),
                title: self.campaign.title.clone(),
                score: self.score.max(0),
                max_score: self.campaign.max_score,
            },
            shift: ShiftView {
                id: self.campaign.shift.id.clone(),
                title: self.campaign.shift.title.clone(),
                status: status.to_owned(),
                elapsed_ms: self.elapsed_ms,
                duration_ms: self.duration_ms(),
                remaining_ms: self.duration_ms().saturating_sub(self.elapsed_ms),
                duty: self
                    .campaign
                    .shift
                    .duties
                    .iter()
                    .find(|duty| self.elapsed_ms >= duty.start_ms && self.elapsed_ms < duty.end_ms)
                    .map(|duty| DutyView {
                        id: duty.id.clone(),
                        chapter: duty.chapter,
                        number: duty.number,
                        city: duty.city.clone(),
                        map_id: duty.map_id.clone(),
                        elapsed_ms: self.elapsed_ms.saturating_sub(duty.start_ms),
                        remaining_ms: duty.end_ms.saturating_sub(self.elapsed_ms),
                    }),
            },
            active_call: self
                .calls
                .iter()
                .position(|call| call.phase == CallPhase::Active)
                .map(|index| self.campaign.shift.calls[index].id.clone()),
            calls,
            incidents,
            units,
            alarms: self
                .alarms
                .iter()
                .filter(|alarm| matches!(alarm.phase, AlarmPhase::Pending | AlarmPhase::Due))
                .map(alarm_view)
                .collect(),
            next_alarm_ms: self.next_pending_alarm_ms(),
            controls: vec![
                "start", "show", "answer", "say", "dispatch", "recall", "alarm", "cancel", "wait",
                "submit",
            ],
        }
    }

    pub fn final_score(&mut self) -> i64 {
        if self.started {
            self.advance_to(self.duration_ms())
                .expect("advancing to a valid final deadline cannot fail");
        }
        self.score.max(0)
    }

    fn require_running(&self) -> Result<(), String> {
        if !self.started {
            return Err("start the shift first".to_owned());
        }
        if self.elapsed_ms >= self.duration_ms() {
            return Err("the shift is complete".to_owned());
        }
        Ok(())
    }

    fn call_index(&self, id: &str) -> Result<usize, String> {
        self.campaign
            .shift
            .calls
            .iter()
            .position(|call| call.id == id)
            .ok_or_else(|| format!("unknown call {id:?}"))
    }

    fn incident_index(&self, id: &str) -> Result<usize, String> {
        self.incidents
            .iter()
            .position(|runtime| {
                self.campaign.shift.calls[runtime.call_index]
                    .incident
                    .as_ref()
                    .is_some_and(|incident| incident.id == id)
            })
            .ok_or_else(|| format!("unknown incident {id:?}"))
    }

    fn incident_for_call(&self, call_index: usize) -> Option<usize> {
        self.incidents
            .iter()
            .position(|incident| incident.call_index == call_index)
    }

    fn unit_index(&self, id: &str) -> Result<usize, String> {
        self.campaign
            .shift
            .units
            .iter()
            .position(|unit| unit.id == id)
            .ok_or_else(|| format!("unknown unit {id:?}"))
    }

    fn travel_ms(&self, unit_index: usize, incident_index: usize) -> u64 {
        let unit = &self.campaign.shift.units[unit_index];
        let call = &self.campaign.shift.calls[self.incidents[incident_index].call_index];
        let incident = call
            .incident
            .as_ref()
            .expect("runtime incident has definition");
        let numerator = unit.base.distance(incident.location).saturating_mul(60_000);
        numerator
            .saturating_add(unit.speed_cells_per_minute - 1)
            .checked_div(unit.speed_cells_per_minute)
            .unwrap_or(0)
            .max(1)
    }

    fn next_boundary(&self, target_ms: u64) -> u64 {
        let mut next = target_ms.min(self.duration_ms());
        for (definition, runtime) in self.campaign.shift.calls.iter().zip(&self.calls) {
            match runtime.phase {
                CallPhase::Scheduled => next = next.min(definition.arrival_ms),
                CallPhase::Ringing => next = next.min(runtime.answer_deadline_ms),
                CallPhase::Active => {
                    if let Some(deadline) = runtime.conversation_deadline_ms {
                        next = next.min(deadline);
                    }
                }
                _ => {}
            }
        }
        for phase in &self.units {
            match phase {
                UnitPhase::EnRoute { arrival_ms, .. } | UnitPhase::Returning { arrival_ms } => {
                    next = next.min(*arrival_ms)
                }
                _ => {}
            }
        }
        for alarm in &self.alarms {
            if alarm.phase == AlarmPhase::Pending {
                next = next.min(alarm.due_ms);
            }
        }
        for (incident_index, incident) in self.incidents.iter().enumerate() {
            if self.calls[incident.call_index].phase == CallPhase::Scheduled {
                continue;
            }
            if matches!(
                incident.phase,
                IncidentPhase::Resolved | IncidentPhase::Lost
            ) {
                continue;
            }
            for element in incident
                .elements
                .iter()
                .filter(|element| element.active && element.kind != SceneKind::Dead)
            {
                if let Some(remaining) = element.remaining_timer_ms
                    && remaining > 0
                {
                    next = next.min(self.elapsed_ms.saturating_add(remaining));
                }
                if let Some(health) = element.health_milli
                    && element.health_decay_milli_per_minute > 0
                {
                    let numerator = health
                        .saturating_mul(60_000)
                        .saturating_sub(element.health_remainder)
                        .max(1) as u64;
                    let rate = element.health_decay_milli_per_minute as u64;
                    let delta = numerator.saturating_add(rate - 1) / rate;
                    next = next.min(self.elapsed_ms.saturating_add(delta));
                }
            }
            for role in [Role::Police, Role::Fire, Role::Medical] {
                let workers = self.workers(incident_index, role);
                let remaining = incident
                    .elements
                    .iter()
                    .filter(|element| {
                        element.active
                            && element.role == Some(role)
                            && scene_element_unblocked(incident, element)
                    })
                    .fold(0_u64, |sum, element| {
                        sum.saturating_add(element.remaining_work_ms)
                    });
                if workers > 0 && remaining > 0 {
                    let delta = remaining.saturating_add(workers - 1) / workers;
                    next = next.min(self.elapsed_ms.saturating_add(delta));
                }
            }
        }
        next
    }

    fn advance_continuous(&mut self, delta_ms: u64) {
        for incident_index in 0..self.incidents.len() {
            if self.calls[self.incidents[incident_index].call_index].phase == CallPhase::Scheduled {
                continue;
            }
            if matches!(
                self.incidents[incident_index].phase,
                IncidentPhase::Resolved | IncidentPhase::Lost
            ) {
                continue;
            }
            let workers = [
                (Role::Police, self.workers(incident_index, Role::Police)),
                (Role::Fire, self.workers(incident_index, Role::Fire)),
                (Role::Medical, self.workers(incident_index, Role::Medical)),
            ];
            let incident = &mut self.incidents[incident_index];
            for element in incident
                .elements
                .iter_mut()
                .filter(|element| element.active)
            {
                if let Some(remaining) = &mut element.remaining_timer_ms {
                    *remaining = remaining.saturating_sub(delta_ms);
                }
                if let Some(health) = &mut element.health_milli {
                    let decay = element
                        .health_decay_milli_per_minute
                        .saturating_mul(delta_ms as i64)
                        .saturating_add(element.health_remainder);
                    *health -= decay / 60_000;
                    element.health_remainder = decay % 60_000;
                }
                let growth = element
                    .work_growth_ms_per_minute
                    .saturating_mul(delta_ms as i64)
                    .saturating_add(element.work_remainder);
                let work_delta = (growth / 60_000).max(0) as u64;
                element.work_remainder = growth % 60_000;
                element.remaining_work_ms = element.remaining_work_ms.saturating_add(work_delta);
                element.total_work_ms = element.total_work_ms.saturating_add(work_delta);
            }
            let blocked = incident
                .elements
                .iter()
                .filter(|element| element.active && element.remaining_work_ms > 0)
                .map(|element| element.id.clone())
                .collect::<HashSet<_>>();
            for (role, worker_count) in workers {
                let mut capacity = delta_ms.saturating_mul(worker_count);
                for element in incident.elements.iter_mut().filter(|element| {
                    element.active
                        && element.role == Some(role)
                        && element
                            .blocked_by
                            .as_ref()
                            .is_none_or(|blocker| !blocked.contains(blocker))
                }) {
                    let applied = capacity.min(element.remaining_work_ms);
                    element.remaining_work_ms -= applied;
                    capacity -= applied;
                    if capacity == 0 {
                        break;
                    }
                }
            }
        }
    }

    fn workers(&self, incident_index: usize, role: Role) -> u64 {
        self.units
            .iter()
            .enumerate()
            .filter(|(unit_index, phase)| {
                self.campaign.shift.units[*unit_index].role == role
                    && matches!(phase, UnitPhase::OnScene { incident_index: index } if *index == incident_index)
            })
            .count() as u64
    }

    fn process_due(&mut self) {
        for (index, definition) in self.campaign.shift.calls.iter().enumerate() {
            let incident_index = self
                .incidents
                .iter()
                .position(|incident| incident.call_index == index);
            if self.calls[index].phase == CallPhase::Scheduled
                && definition.arrival_ms <= self.elapsed_ms
            {
                if definition.kind == EventKind::Report {
                    self.calls[index].phase = CallPhase::Completed;
                    if let Some(incident_index) = incident_index {
                        self.incidents[incident_index].phase = IncidentPhase::Reported;
                    }
                } else {
                    self.calls[index].phase = CallPhase::Ringing;
                }
            }
            let runtime = &mut self.calls[index];
            if runtime.phase == CallPhase::Ringing && runtime.answer_deadline_ms <= self.elapsed_ms
            {
                runtime.phase = CallPhase::Missed;
            }
            if runtime.phase == CallPhase::Active
                && runtime
                    .conversation_deadline_ms
                    .is_some_and(|deadline| deadline <= self.elapsed_ms)
            {
                runtime.phase = CallPhase::Dropped;
                runtime.current_stage = None;
                runtime.conversation_deadline_ms = None;
                if let Some(opinion) = runtime.ignore_opinion_milli.take() {
                    self.score += opinion / 100;
                }
            }
        }
        for phase in &mut self.units {
            match *phase {
                UnitPhase::EnRoute {
                    incident_index,
                    arrival_ms,
                } if arrival_ms <= self.elapsed_ms => {
                    *phase = UnitPhase::OnScene { incident_index };
                }
                UnitPhase::Returning { arrival_ms } if arrival_ms <= self.elapsed_ms => {
                    *phase = UnitPhase::Idle;
                }
                _ => {}
            }
        }
        for alarm in &mut self.alarms {
            if alarm.phase == AlarmPhase::Pending && alarm.due_ms <= self.elapsed_ms {
                alarm.phase = AlarmPhase::Due;
            }
        }

        self.process_scene_completions();

        let mut terminal_incidents = Vec::new();
        for (index, incident) in self.incidents.iter_mut().enumerate() {
            if matches!(
                incident.phase,
                IncidentPhase::Resolved | IncidentPhase::Lost
            ) {
                continue;
            }
            let failed = incident.elements.iter().any(|element| {
                element.active
                    && element.kind != SceneKind::Dead
                    && element.health_milli.is_some_and(|health| health <= 0)
            });
            let resolved = incident.phase == IncidentPhase::Reported
                && incident.elements.iter().all(|element| {
                    !element.active
                        || ((element.role.is_none() || element.remaining_work_ms == 0)
                            && (element.remaining_timer_ms.is_none() || element.completed))
                });
            if failed {
                for element in &mut incident.elements {
                    if element.health_milli.is_some_and(|health| health < 0) {
                        element.health_milli = Some(0);
                    }
                }
                incident.phase = IncidentPhase::Lost;
                terminal_incidents.push(index);
            } else if resolved {
                incident.phase = IncidentPhase::Resolved;
                let definition = self.campaign.shift.calls[incident.call_index]
                    .incident
                    .as_ref()
                    .expect("runtime incident has definition");
                self.score += definition.base_score + incident_health(incident) / 1_000;
                terminal_incidents.push(index);
            }
        }
        for incident_index in terminal_incidents {
            for unit_index in 0..self.units.len() {
                let assigned = matches!(
                    self.units[unit_index],
                    UnitPhase::EnRoute { incident_index: index, .. }
                        | UnitPhase::OnScene { incident_index: index }
                        if index == incident_index
                );
                if assigned {
                    let arrival_ms = self
                        .elapsed_ms
                        .saturating_add(self.travel_ms(unit_index, incident_index));
                    self.units[unit_index] = UnitPhase::Returning { arrival_ms };
                }
            }
        }

        if self.elapsed_ms >= self.duration_ms() {
            for call in &mut self.calls {
                match call.phase {
                    CallPhase::Scheduled | CallPhase::Ringing => call.phase = CallPhase::Missed,
                    CallPhase::Active => call.phase = CallPhase::Dropped,
                    _ => {}
                }
            }
            for incident in &mut self.incidents {
                if !matches!(
                    incident.phase,
                    IncidentPhase::Resolved | IncidentPhase::Lost
                ) {
                    incident.phase = IncidentPhase::Lost;
                }
            }
        }
    }

    fn process_scene_completions(&mut self) {
        loop {
            let pending =
                self.incidents
                    .iter()
                    .enumerate()
                    .find_map(|(incident_index, incident)| {
                        if matches!(
                            incident.phase,
                            IncidentPhase::Resolved | IncidentPhase::Lost
                        ) || self.calls[incident.call_index].phase == CallPhase::Scheduled
                        {
                            return None;
                        }
                        incident
                            .elements
                            .iter()
                            .enumerate()
                            .find(|(_, element)| {
                                element.active
                                    && !element.completed
                                    && (element.remaining_timer_ms == Some(0)
                                        || (element.total_work_ms > 0
                                            && element.remaining_work_ms == 0))
                            })
                            .map(|(element_index, _)| {
                                (incident_index, element_index, incident.call_index)
                            })
                    });
            let Some((incident_index, element_index, call_index)) = pending else {
                break;
            };
            let element = &mut self.incidents[incident_index].elements[element_index];
            element.completed = true;
            if element.remaining_timer_ms.is_some() {
                element.active = false;
            }
            let actions = element.actions.clone();
            let aar = element.aar.clone();
            self.calls[call_index].aar.extend(aar);
            self.apply_graph_actions(call_index, &actions)
                .expect("validated scene completion actions");
        }
    }

    fn call_view(
        &self,
        call_index: usize,
        definition: &CallDefinition,
        runtime: &CallRuntime,
    ) -> CallView {
        let (caller_text, choices) = if runtime.phase == CallPhase::Active {
            runtime
                .current_stage
                .as_deref()
                .and_then(|id| definition.node(id).map(|node| (id, node)))
                .map(|(id, node)| {
                    let choices = self
                        .graph_answers(call_index, id)
                        .unwrap_or_default()
                        .into_iter()
                        .filter_map(|answer| {
                            definition.node(&answer).map(|target| ChoiceView {
                                id: target.id.clone(),
                                text: button_text(&target.text),
                            })
                        })
                        .collect();
                    (Some(conversation_text(&node.text)), choices)
                })
                .unwrap_or((None, Vec::new()))
        } else {
            (None, Vec::new())
        };
        CallView {
            id: definition.id.clone(),
            caller: definition.caller.clone(),
            status: enum_name(runtime.phase),
            answer_deadline_ms: (runtime.phase == CallPhase::Ringing)
                .then_some(runtime.answer_deadline_ms),
            conversation_deadline_ms: runtime.conversation_deadline_ms,
            caller_text,
            choices,
            transcript: runtime
                .history
                .iter()
                .filter_map(|id| definition.node(id))
                .map(|node| TranscriptView {
                    speaker: if node.operator { "operator" } else { "caller" },
                    text: conversation_text(&node.text),
                })
                .collect(),
            opinion_milli: runtime.opinion_milli,
            after_action_report: runtime.aar.clone(),
            facts: runtime.facts.clone(),
        }
    }

    fn incident_view(&self, runtime: &IncidentRuntime) -> IncidentView {
        let definition = self.campaign.shift.calls[runtime.call_index]
            .incident
            .as_ref()
            .expect("runtime incident has definition");
        let requirements = [Role::Police, Role::Fire, Role::Medical]
            .into_iter()
            .filter_map(|role| {
                let elements = runtime
                    .elements
                    .iter()
                    .filter(|element| element.active && element.role == Some(role))
                    .collect::<Vec<_>>();
                (!elements.is_empty()).then(|| RequirementView {
                    role,
                    remaining_work_ms: elements.iter().fold(0_u64, |sum, element| {
                        sum.saturating_add(element.remaining_work_ms)
                    }),
                    total_work_ms: elements.iter().fold(0_u64, |sum, element| {
                        sum.saturating_add(element.total_work_ms)
                    }),
                })
            })
            .collect();
        IncidentView {
            id: definition.id.clone(),
            title: definition.title.clone(),
            status: enum_name(runtime.phase),
            location: definition.location,
            health_milli: incident_health(runtime),
            health_decay_milli_per_minute: runtime
                .elements
                .iter()
                .filter(|element| element.active)
                .map(|element| element.health_decay_milli_per_minute)
                .max()
                .unwrap_or(0),
            requirements,
            elements: runtime
                .elements
                .iter()
                .map(|element| SceneElementView {
                    id: element.id.clone(),
                    label: element.label.clone(),
                    kind: element.kind,
                    active: element.active,
                    role: element.role,
                    health_milli: element.health_milli,
                    health_decay_milli_per_minute: element.health_decay_milli_per_minute,
                    remaining_work_ms: element.remaining_work_ms,
                    total_work_ms: element.total_work_ms,
                    blocked_by: element.blocked_by.clone(),
                    weapon: element.weapon.clone(),
                    fight_risk_milli: element.fight_risk_milli,
                    prison_chance_milli: element.prison_chance_milli,
                    bill: element.bill,
                    remaining_timer_ms: element.remaining_timer_ms,
                })
                .collect(),
        }
    }

    fn unit_view(&self, id: &str, phase: &UnitPhase) -> UnitView {
        let definition = self
            .campaign
            .shift
            .units
            .iter()
            .find(|unit| unit.id == id)
            .expect("unit runtime has definition");
        let (status, incident, eta_ms) = match phase {
            UnitPhase::Idle => ("idle", None, None),
            UnitPhase::EnRoute {
                incident_index,
                arrival_ms,
            } => (
                "en_route",
                Some(self.incident_id(*incident_index)),
                Some(arrival_ms.saturating_sub(self.elapsed_ms)),
            ),
            UnitPhase::OnScene { incident_index } => {
                ("on_scene", Some(self.incident_id(*incident_index)), None)
            }
            UnitPhase::Returning { arrival_ms } => (
                "returning",
                None,
                Some(arrival_ms.saturating_sub(self.elapsed_ms)),
            ),
        };
        UnitView {
            id: definition.id.clone(),
            label: definition.label.clone(),
            role: definition.role,
            status: status.to_owned(),
            incident,
            eta_ms,
            location: match phase {
                UnitPhase::EnRoute { incident_index, .. }
                | UnitPhase::OnScene { incident_index } => {
                    let call_index = self.incidents[*incident_index].call_index;
                    self.campaign.shift.calls[call_index]
                        .incident
                        .as_ref()
                        .expect("runtime incident has definition")
                        .location
                }
                _ => definition.base,
            },
        }
    }

    fn incident_id(&self, index: usize) -> String {
        self.campaign.shift.calls[self.incidents[index].call_index]
            .incident
            .as_ref()
            .expect("runtime incident has definition")
            .id
            .clone()
    }
}

fn runtime_elements(incident: &IncidentDefinition) -> Vec<SceneElementRuntime> {
    incident
        .elements
        .iter()
        .map(|element| SceneElementRuntime {
            id: element.id.clone(),
            label: element.label.clone(),
            kind: element.kind,
            active: element.active,
            role: element.role,
            health_milli: element.health_milli,
            health_decay_milli_per_minute: element.health_decay_milli_per_minute,
            health_remainder: 0,
            remaining_work_ms: element.work_ms,
            total_work_ms: element.work_ms,
            work_growth_ms_per_minute: element.work_growth_ms_per_minute,
            work_remainder: 0,
            blocked_by: element.blocked_by.clone(),
            weapon: element.weapon.clone(),
            fight_risk_milli: element.fight_risk_milli,
            prison_chance_milli: element.prison_chance_milli,
            bill: element.bill,
            remaining_timer_ms: element.timer_ms,
            actions: element.actions.clone(),
            aar: element.aar.clone(),
            completed: element.timer_ms.is_none() && element.work_ms == 0,
        })
        .collect()
}

fn incident_health(incident: &IncidentRuntime) -> i64 {
    incident
        .elements
        .iter()
        .filter(|element| element.active && element.kind != SceneKind::Dead)
        .filter_map(|element| element.health_milli)
        .min()
        .unwrap_or(100_000)
}

fn scene_element_unblocked(incident: &IncidentRuntime, element: &SceneElementRuntime) -> bool {
    element.blocked_by.as_ref().is_none_or(|blocker| {
        incident
            .elements
            .iter()
            .find(|candidate| &candidate.id == blocker)
            .is_none_or(|candidate| !candidate.active || candidate.remaining_work_ms == 0)
    })
}

const fn role_for_scene(kind: SceneKind) -> Option<Role> {
    match kind {
        SceneKind::Criminal | SceneKind::Suspect => Some(Role::Police),
        SceneKind::Injured => Some(Role::Medical),
        SceneKind::Fire | SceneKind::Tech | SceneKind::Work => Some(Role::Fire),
        _ => None,
    }
}

fn parse_scene_kind(value: &str) -> Option<SceneKind> {
    match value {
        "criminal" => Some(SceneKind::Criminal),
        "suspect" => Some(SceneKind::Suspect),
        "injured" | "injuried" => Some(SceneKind::Injured),
        "dead" => Some(SceneKind::Dead),
        "fire" => Some(SceneKind::Fire),
        "tech" => Some(SceneKind::Tech),
        "work" => Some(SceneKind::Work),
        "timer" => Some(SceneKind::Timer),
        "witness" => Some(SceneKind::Witness),
        "passerby" => Some(SceneKind::Passerby),
        "deco" => Some(SceneKind::Deco),
        _ => None,
    }
}

fn enum_name<T: Serialize>(value: T) -> String {
    serde_json::to_value(value)
        .expect("serializable enum")
        .as_str()
        .expect("unit enum serializes to string")
        .to_owned()
}

fn alarm_view(alarm: &AlarmRuntime) -> AlarmView {
    AlarmView {
        id: alarm.id.clone(),
        note: alarm.note.clone(),
        due_ms: alarm.due_ms,
        status: enum_name(alarm.phase),
    }
}

fn button_text(text: &str) -> String {
    text.find('{')
        .and_then(|start| {
            text[start + 1..]
                .find('}')
                .map(|end| &text[start + 1..start + 1 + end])
        })
        .map_or_else(|| conversation_text(text), |label| label.to_owned())
}

fn conversation_text(text: &str) -> String {
    let mut output = String::with_capacity(text.len());
    let mut inside_label = false;
    for character in text.chars() {
        match character {
            '{' => inside_label = true,
            '}' if inside_label => inside_label = false,
            _ if !inside_label => output.push(character),
            _ => {}
        }
    }
    output.trim().to_owned()
}

fn parse_bool(value: &str) -> Result<bool, String> {
    match value {
        "true" => Ok(true),
        "false" => Ok(false),
        _ => Err(format!("invalid boolean value {value:?}")),
    }
}

fn apply_number(current: i64, assignment: &str, scale: i64) -> Result<i64, String> {
    let (operation, raw) = if let Some(value) = assignment.strip_prefix("+=") {
        ("add", value)
    } else if let Some(value) = assignment.strip_prefix("-=") {
        ("subtract", value)
    } else if let Some(value) = assignment.strip_prefix('=') {
        ("set", value)
    } else {
        return Err(format!("invalid numeric assignment {assignment:?}"));
    };
    let value = raw
        .parse::<f64>()
        .map_err(|_| format!("invalid numeric value {raw:?}"))?;
    if !value.is_finite() {
        return Err(format!("non-finite numeric value {raw:?}"));
    }
    let scaled = (value * scale as f64).round();
    if scaled < i64::MIN as f64 || scaled > i64::MAX as f64 {
        return Err(format!("numeric value {raw:?} is out of range"));
    }
    let scaled = scaled as i64;
    Ok(match operation {
        "add" => current.saturating_add(scaled),
        "subtract" => current.saturating_sub(scaled),
        _ => scaled,
    })
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use serde_json::json;

    use super::*;
    use crate::campaign::CAMPAIGN_SCHEMA;

    fn campaign() -> Campaign {
        Campaign::load(Path::new(env!("CARGO_MANIFEST_DIR")).join("data/campaign/pilot.json"))
            .expect("campaign")
    }

    fn graph_campaign() -> Campaign {
        let campaign: Campaign = serde_json::from_value(json!({
            "schema": CAMPAIGN_SCHEMA,
            "id": "graph-test",
            "title": "Graph test",
            "seed": 1,
            "max_score": 100,
            "shift": {
                "id": "graph",
                "title": "Graph",
                "duration_ms": 600_000,
                "units": [{
                    "id": "medic",
                    "label": "Medic",
                    "role": "medical",
                    "base": {"x": 0, "y": 0},
                    "speed_cells_per_minute": 1
                }],
                "calls": [{
                    "id": "call",
                    "caller": "Caller",
                    "arrival_ms": 0,
                    "answer_window_ms": 60_000,
                    "conversation_window_ms": 300_000,
                    "initial_stage": "1",
                    "disabled_nodes": ["hidden"],
                    "nodes": [
                        {
                            "id": "1",
                            "operator": true,
                            "text": "911",
                            "answers": ["2"]
                        },
                        {
                            "id": "2",
                            "operator": false,
                            "text": "Help",
                            "answers": ["location", "unlock"]
                        },
                        {
                            "id": "location",
                            "operator": true,
                            "text": "{WHERE?} Where are you?",
                            "answers": ["done"],
                            "actions": ["actionSetLocation"]
                        },
                        {
                            "id": "unlock",
                            "operator": true,
                            "text": "{DETAILS} Tell me more",
                            "answers": ["3"],
                            "actions": ["hidden -> active = true"]
                        },
                        {
                            "id": "3",
                            "operator": false,
                            "text": "There is an injured person",
                            "answers": ["hidden", "back"]
                        },
                        {
                            "id": "hidden",
                            "operator": true,
                            "text": "{FIRST AID} Give first aid",
                            "answers": ["done"],
                            "actions": [
                                "actionSetLocation; opinionEffect += 1.5; victim.hpChange = -0.5; victim.bill = 2000; speed = 50; dir = N"
                            ],
                            "aar": ["first-aid"]
                        },
                        {
                            "id": "done",
                            "operator": false,
                            "text": "Thank you",
                            "actions": ["actionHangup"]
                        }
                    ],
                    "incident": {
                        "id": "incident",
                        "title": "Injury",
                        "location": {"x": 1, "y": 1},
                        "base_score": 10,
                        "elements": [{
                            "id": "victim",
                            "label": "Patient",
                            "kind": "injured",
                            "active": true,
                            "role": "medical",
                            "health_milli": 100_000,
                            "health_decay_milli_per_minute": 0,
                            "work_ms": 60_000
                        }]
                    }
                }]
            }
        }))
        .expect("graph campaign syntax");
        campaign.validate().expect("graph campaign");
        campaign
    }

    #[test]
    fn clock_is_irreversible_and_calls_arrive_without_agent_actions() {
        let campaign = campaign();
        let mut session = Session::new(&campaign);
        session.start().expect("start");
        assert_eq!(session.snapshot().calls[0].status, "ringing");
        session.advance_to(301_000).expect("advance");
        assert_eq!(session.snapshot().calls[0].status, "missed");
        assert!(session.advance_to(300_000).is_err());
    }

    #[test]
    fn future_incidents_do_not_decay_before_their_calls_arrive() {
        let campaign = campaign();
        let mut session = Session::new(&campaign);
        session.start().expect("start");
        session.advance_to(300_000).expect("advance to fire call");
        session.answer("fire-1").expect("answer fire call");
        session
            .say("fire-1", "location")
            .expect("reveal fire incident");
        let fire = session
            .snapshot()
            .incidents
            .into_iter()
            .find(|incident| incident.id == "apartment-fire")
            .expect("fire incident");
        assert_eq!(fire.health_milli, 100_000);
    }

    #[test]
    fn an_alarm_only_wakes_and_never_dispatches() {
        let campaign = campaign();
        let mut session = Session::new(&campaign);
        session.start().expect("start");
        session
            .set_alarm("check", 60_000, "check first call")
            .expect("alarm");
        session.advance_to(60_000).expect("advance");
        assert_eq!(session.deliver_due_alarms().len(), 1);
        assert!(
            session
                .snapshot()
                .units
                .iter()
                .all(|unit| unit.status == "idle")
        );
    }

    #[test]
    fn dialogue_reveals_an_incident_and_units_resolve_it_over_time() {
        let campaign = campaign();
        let mut session = Session::new(&campaign);
        session.start().expect("start");
        session.answer("medical-1").expect("answer");
        session.say("medical-1", "address").expect("ask address");
        session
            .say("medical-1", "pressure")
            .expect("give instruction");
        let arrival = session
            .dispatch("medic-1", "bleeding-worker")
            .expect("dispatch");
        session.advance_to(arrival + 240_000).expect("resolve");
        assert_eq!(session.snapshot().incidents[0].status, "resolved");
        assert!(session.snapshot().campaign.score > 100);
    }

    #[test]
    fn final_score_advances_an_abandoned_run_to_the_shift_deadline() {
        let campaign = campaign();
        let mut session = Session::new(&campaign);
        session.start().expect("start");
        assert_eq!(session.final_score(), 0);
        assert_eq!(session.snapshot().shift.status, "complete");
    }

    #[test]
    fn career_snapshot_drops_inactive_history_at_the_terminal_boundary() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let campaign = Campaign::load(root.join("data/campaign/911-career.json"))
            .expect("compiled 911 Operator career");
        let mut session = Session::new(&campaign);
        session.start().expect("start");
        session
            .advance_to(campaign.shift.duration_ms)
            .expect("advance to terminal boundary");

        let snapshot = session.snapshot();
        assert_eq!(snapshot.shift.status, "complete");
        assert!(snapshot.calls.is_empty());
        assert!(snapshot.incidents.is_empty());
        assert!(snapshot.alarms.is_empty());
        assert!(
            serde_json::to_vec(&snapshot)
                .expect("serialize compact snapshot")
                .len()
                < 4_000
        );
    }

    #[test]
    fn graph_dialogue_matches_original_option_and_action_semantics() {
        let campaign = graph_campaign();
        let mut session = Session::new(&campaign);
        session.start().expect("start");
        session.answer("call").expect("answer");

        let call = &session.snapshot().calls[0];
        assert_eq!(call.caller_text.as_deref(), Some("Help"));
        assert_eq!(
            call.choices
                .iter()
                .map(|choice| (choice.id.as_str(), choice.text.as_str()))
                .collect::<Vec<_>>(),
            vec![("location", "WHERE?"), ("unlock", "DETAILS")]
        );

        session.say("call", "unlock").expect("unlock details");
        let call = &session.snapshot().calls[0];
        assert_eq!(
            call.caller_text.as_deref(),
            Some("There is an injured person")
        );
        assert_eq!(
            call.choices
                .iter()
                .map(|choice| choice.id.as_str())
                .collect::<Vec<_>>(),
            vec!["hidden", "location"]
        );
        assert!(session.say("call", "unlock").is_err());

        session.say("call", "hidden").expect("give first aid");
        let snapshot = session.snapshot();
        assert_eq!(snapshot.calls[0].status, "completed");
        assert_eq!(snapshot.campaign.score, 15);
        assert_eq!(snapshot.incidents[0].status, "reported");
        assert_eq!(snapshot.incidents[0].health_decay_milli_per_minute, 30_000);
        assert_eq!(
            snapshot.incidents[0].elements[0].health_decay_milli_per_minute,
            30_000
        );
        assert_eq!(snapshot.incidents[0].elements[0].bill, Some(2_000));
        assert_eq!(
            snapshot.calls[0].facts.get("speed").map(String::as_str),
            Some("50")
        );
        assert_eq!(
            snapshot.calls[0].facts.get("dir").map(String::as_str),
            Some("n")
        );
        assert_eq!(session.calls[0].aar, vec!["first-aid"]);
    }

    #[test]
    fn every_imported_career_call_can_enter_its_dialogue_graph() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let campaign = Campaign::load(root.join("data/campaign/911-career.json"))
            .expect("compiled 911 Operator career");
        for (index, definition) in campaign.shift.calls.iter().enumerate() {
            if definition.kind == EventKind::Report {
                continue;
            }
            let mut session = Session::new(&campaign);
            session.start().expect("start");
            session
                .advance_to(definition.arrival_ms)
                .expect("advance to call");
            session
                .answer(&definition.id)
                .unwrap_or_else(|error| panic!("call {} failed: {error}", definition.id));
            if session.calls[index].phase == CallPhase::Active {
                let node = definition
                    .node(
                        session.calls[index]
                            .current_stage
                            .as_deref()
                            .expect("active call node"),
                    )
                    .expect("known active call node");
                assert!(
                    !node.operator,
                    "{} stopped on an operator node",
                    definition.id
                );
            }
        }
    }

    #[test]
    fn imported_duties_publish_reports_without_automating_a_response() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let campaign = Campaign::load(root.join("data/campaign/911-career.json"))
            .expect("compiled 911 Operator career");
        let first_report = campaign
            .shift
            .calls
            .iter()
            .find(|event| event.kind == EventKind::Report)
            .expect("generated report");
        let mut session = Session::new(&campaign);
        session.start().expect("start");
        session
            .advance_to(first_report.arrival_ms)
            .expect("advance to report");
        let snapshot = session.snapshot();
        let duty = snapshot.shift.duty.expect("active duty");
        assert_eq!(duty.city, "Kapolei");
        assert!(snapshot.incidents.iter().any(|incident| {
            incident.id == first_report.incident.as_ref().expect("report incident").id
                && incident.status == "reported"
        }));
        assert!(snapshot.units.iter().all(|unit| unit.status == "idle"));
    }

    #[test]
    fn imported_scene_timers_and_completion_actions_follow_the_world_clock() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let campaign = Campaign::load(root.join("data/campaign/911-career.json"))
            .expect("compiled 911 Operator career");
        let call_id = "chapter-1-duty-1-call-2-81";
        let call_index = campaign
            .shift
            .calls
            .iter()
            .position(|call| call.id == call_id)
            .expect("small car fire call");
        let call = &campaign.shift.calls[call_index];
        let incident = call.incident.as_ref().expect("small car fire incident");
        let fire = incident
            .elements
            .iter()
            .find(|element| element.id == "fire")
            .expect("fire work");
        let work_growth_ms_per_minute =
            u64::try_from(fire.work_growth_ms_per_minute).expect("non-negative fire growth");
        let net_work_per_minute = 60_000_u64
            .checked_sub(work_growth_ms_per_minute)
            .expect("one fire crew can make progress");
        let fire_completion_ms = fire
            .work_ms
            .saturating_mul(60_000)
            .div_ceil(net_work_per_minute);
        let smoke_timer_ms = incident
            .elements
            .iter()
            .find(|element| element.id == "smoke")
            .and_then(|element| element.timer_ms)
            .expect("smoke timer");
        let incident_index = Session::new(&campaign)
            .incidents
            .iter()
            .position(|incident| incident.call_index == call_index)
            .expect("small car fire incident");

        let mut expired = Session::new(&campaign);
        expired.start().expect("start");
        expired
            .advance_to(call.arrival_ms + smoke_timer_ms)
            .expect("expire smoke timer");
        let caller = expired.incidents[incident_index]
            .elements
            .iter()
            .find(|element| element.id == "caller")
            .expect("caller element");
        let smoke = expired.incidents[incident_index]
            .elements
            .iter()
            .find(|element| element.id == "smoke")
            .expect("smoke timer");
        assert_eq!(caller.health_milli, Some(80_000));
        assert!(!smoke.active);
        assert!(smoke.completed);
        assert_eq!(expired.score, -20);
        assert!(
            expired.calls[call_index]
                .aar
                .iter()
                .any(|line| line.contains("poisoned by smoke"))
        );

        let mut extinguished = Session::new(&campaign);
        extinguished.start().expect("start");
        extinguished
            .advance_to(call.arrival_ms)
            .expect("fire call arrival");
        extinguished.incidents[incident_index].phase = IncidentPhase::Reported;
        let fire_unit = campaign
            .shift
            .units
            .iter()
            .position(|unit| unit.role == Role::Fire)
            .expect("fire unit");
        extinguished.units[fire_unit] = UnitPhase::OnScene { incident_index };
        extinguished
            .advance_to(call.arrival_ms + fire_completion_ms)
            .expect("extinguish before smoke timer");
        let smoke = extinguished.incidents[incident_index]
            .elements
            .iter()
            .find(|element| element.id == "smoke")
            .expect("smoke timer");
        assert!(!smoke.active);
        assert!(!smoke.completed);
        assert!(
            extinguished.calls[call_index]
                .aar
                .iter()
                .any(|line| line.contains("fire was extinguished"))
        );
        assert!(
            !extinguished.calls[call_index]
                .aar
                .iter()
                .any(|line| line.contains("poisoned by smoke"))
        );
    }

    #[test]
    fn packaged_oracle_opening_path_is_a_positive_full_campaign_replay() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let campaign = Campaign::load(root.join("data/campaign/911-career.json"))
            .expect("compiled 911 Operator career");
        let call = "chapter-1-duty-1-call-1-97";
        let incident = "chapter-1-duty-1-call-1-97-incident";
        let mut session = Session::new(&campaign);
        session.start().expect("start");
        let arrival_ms = campaign
            .shift
            .calls
            .iter()
            .find(|definition| definition.id == call)
            .expect("packaged call")
            .arrival_ms;
        session.advance_to(arrival_ms).expect("first call arrival");
        session.answer(call).expect("answer first call");
        for choice in ["3", "address", "7c", "7e", "9", "11", "13", "20", "17"] {
            session
                .say(call, choice)
                .unwrap_or_else(|error| panic!("choice {choice}: {error}"));
        }
        assert_eq!(session.snapshot().calls[0].status, "completed");
        session
            .dispatch("medical-1", incident)
            .expect("dispatch medic");
        assert!(session.final_score() > 0);
    }
}
