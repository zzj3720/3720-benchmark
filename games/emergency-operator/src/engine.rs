use serde::Serialize;

use crate::campaign::{CallDefinition, Campaign, Point, Role};

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
struct RequirementRuntime {
    role: Role,
    remaining_work_ms: u64,
    total_work_ms: u64,
}

#[derive(Clone, Debug)]
struct IncidentRuntime {
    call_index: usize,
    phase: IncidentPhase,
    health_milli: i64,
    health_decay_milli_per_minute: i64,
    health_remainder: i64,
    requirements: Vec<RequirementRuntime>,
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
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ChoiceView {
    pub id: String,
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
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct UnitView {
    pub id: String,
    pub label: String,
    pub role: Role,
    pub status: String,
    pub incident: Option<String>,
    pub eta_ms: Option<u64>,
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
                    health_milli: incident.health_milli,
                    health_decay_milli_per_minute: incident.health_decay_milli_per_minute,
                    health_remainder: 0,
                    requirements: incident
                        .requirements
                        .iter()
                        .map(|requirement| RequirementRuntime {
                            role: requirement.role,
                            remaining_work_ms: requirement.work_ms,
                            total_work_ms: requirement.work_ms,
                        })
                        .collect(),
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
        call.current_stage = Some(definition.initial_stage.clone());
        call.conversation_deadline_ms = Some(conversation_deadline_ms);
        Ok(())
    }

    pub fn say(&mut self, call_id: &str, choice_id: &str) -> Result<(), String> {
        self.require_running()?;
        let call_index = self.call_index(call_id)?;
        let call = &self.calls[call_index];
        if call.phase != CallPhase::Active {
            return Err(format!("call {call_id:?} is not active"));
        }
        let stage_id = call
            .current_stage
            .as_deref()
            .ok_or_else(|| format!("call {call_id:?} has no active dialogue stage"))?;
        let choice = self.campaign.shift.calls[call_index]
            .stage(stage_id)
            .and_then(|stage| stage.choices.iter().find(|choice| choice.id == choice_id))
            .cloned()
            .ok_or_else(|| format!("choice {choice_id:?} is not available"))?;

        self.score += choice.score_delta;
        if let Some(incident_index) = self.incident_for_call(call_index) {
            let incident = &mut self.incidents[incident_index];
            incident.health_decay_milli_per_minute = (incident.health_decay_milli_per_minute
                + choice.health_decay_delta_milli_per_minute)
                .max(0);
            if choice.reveal_incident && incident.phase == IncidentPhase::Hidden {
                incident.phase = IncidentPhase::Reported;
            }
        }

        let call = &mut self.calls[call_index];
        if let Some(next) = choice.next {
            call.current_stage = Some(next);
        } else {
            call.phase = CallPhase::Completed;
            call.current_stage = None;
            call.conversation_deadline_ms = None;
        }
        Ok(())
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
        let calls = self
            .campaign
            .shift
            .calls
            .iter()
            .zip(&self.calls)
            .filter(|(_, runtime)| runtime.phase != CallPhase::Scheduled)
            .map(|(definition, runtime)| self.call_view(definition, runtime))
            .collect();
        let incidents = self
            .incidents
            .iter()
            .enumerate()
            .filter(|(_, runtime)| runtime.phase != IncidentPhase::Hidden)
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
            },
            active_call: self
                .calls
                .iter()
                .position(|call| call.phase == CallPhase::Active)
                .map(|index| self.campaign.shift.calls[index].id.clone()),
            calls,
            incidents,
            units,
            alarms: self.alarms.iter().map(alarm_view).collect(),
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
            if incident.health_decay_milli_per_minute > 0 {
                let numerator = incident
                    .health_milli
                    .saturating_mul(60_000)
                    .saturating_sub(incident.health_remainder)
                    .max(1) as u64;
                let rate = incident.health_decay_milli_per_minute as u64;
                let delta = numerator.saturating_add(rate - 1) / rate;
                next = next.min(self.elapsed_ms.saturating_add(delta));
            }
            for requirement in &incident.requirements {
                let workers = self.workers(incident_index, requirement.role);
                if workers > 0 && requirement.remaining_work_ms > 0 {
                    let delta = requirement.remaining_work_ms.saturating_add(workers - 1) / workers;
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
            let roles = self.incidents[incident_index]
                .requirements
                .iter()
                .map(|requirement| requirement.role)
                .collect::<Vec<_>>();
            let workers = roles
                .iter()
                .map(|role| self.workers(incident_index, *role))
                .collect::<Vec<_>>();
            let incident = &mut self.incidents[incident_index];
            let decay = incident
                .health_decay_milli_per_minute
                .saturating_mul(delta_ms as i64)
                .saturating_add(incident.health_remainder);
            incident.health_milli -= decay / 60_000;
            incident.health_remainder = decay % 60_000;

            for (requirement, workers) in incident.requirements.iter_mut().zip(workers) {
                requirement.remaining_work_ms = requirement
                    .remaining_work_ms
                    .saturating_sub(delta_ms.saturating_mul(workers));
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
        for (definition, runtime) in self.campaign.shift.calls.iter().zip(&mut self.calls) {
            if runtime.phase == CallPhase::Scheduled && definition.arrival_ms <= self.elapsed_ms {
                runtime.phase = CallPhase::Ringing;
            }
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

        let mut terminal_incidents = Vec::new();
        for (index, incident) in self.incidents.iter_mut().enumerate() {
            if matches!(
                incident.phase,
                IncidentPhase::Resolved | IncidentPhase::Lost
            ) {
                continue;
            }
            if incident.health_milli <= 0 {
                incident.health_milli = 0;
                incident.phase = IncidentPhase::Lost;
                terminal_incidents.push(index);
            } else if incident
                .requirements
                .iter()
                .all(|requirement| requirement.remaining_work_ms == 0)
            {
                incident.phase = IncidentPhase::Resolved;
                let definition = self.campaign.shift.calls[incident.call_index]
                    .incident
                    .as_ref()
                    .expect("runtime incident has definition");
                self.score += definition.base_score + incident.health_milli / 1_000;
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

    fn call_view(&self, definition: &CallDefinition, runtime: &CallRuntime) -> CallView {
        let (caller_text, choices) = if runtime.phase == CallPhase::Active {
            runtime
                .current_stage
                .as_deref()
                .and_then(|id| definition.stage(id))
                .map(|stage| {
                    (
                        Some(stage.caller.clone()),
                        stage
                            .choices
                            .iter()
                            .map(|choice| ChoiceView {
                                id: choice.id.clone(),
                                text: choice.text.clone(),
                            })
                            .collect(),
                    )
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
        }
    }

    fn incident_view(&self, runtime: &IncidentRuntime) -> IncidentView {
        let definition = self.campaign.shift.calls[runtime.call_index]
            .incident
            .as_ref()
            .expect("runtime incident has definition");
        IncidentView {
            id: definition.id.clone(),
            title: definition.title.clone(),
            status: enum_name(runtime.phase),
            location: definition.location,
            health_milli: runtime.health_milli,
            health_decay_milli_per_minute: runtime.health_decay_milli_per_minute,
            requirements: runtime
                .requirements
                .iter()
                .map(|requirement| RequirementView {
                    role: requirement.role,
                    remaining_work_ms: requirement.remaining_work_ms,
                    total_work_ms: requirement.total_work_ms,
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

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    fn campaign() -> Campaign {
        Campaign::load(Path::new(env!("CARGO_MANIFEST_DIR")).join("data/campaign/pilot.json"))
            .expect("campaign")
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
}
