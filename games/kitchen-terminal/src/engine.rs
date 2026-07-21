use serde::Serialize;

use crate::campaign::{Campaign, ComponentDefinition, RecipeDefinition, StationKind};

pub const STATE_SCHEMA: &str = "kitchen-state-v1";

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
enum OrderPhase {
    Scheduled,
    Open,
    Assembled,
    Served,
    Missed,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum ComponentPhase {
    Waiting,
    Cooking {
        station_index: usize,
        ready_ms: u64,
        burn_ms: u64,
    },
    Done,
    Burnt {
        station_index: usize,
    },
}

struct OrderRuntime {
    phase: OrderPhase,
    components: Vec<ComponentPhase>,
    earned_score: i64,
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
pub struct ComponentView {
    pub id: String,
    pub label: String,
    pub station: StationKind,
    pub status: String,
    pub ready_in_ms: Option<u64>,
    pub burn_in_ms: Option<u64>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct OrderView {
    pub id: String,
    pub recipe: String,
    pub title: String,
    pub status: String,
    pub deadline_ms: u64,
    pub remaining_ms: u64,
    pub earned_score: i64,
    pub components: Vec<ComponentView>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct StationView {
    pub id: String,
    pub label: String,
    pub kind: StationKind,
    pub status: String,
    pub order: Option<String>,
    pub component: Option<String>,
    pub ready_in_ms: Option<u64>,
    pub burn_in_ms: Option<u64>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct Snapshot {
    pub schema: &'static str,
    pub campaign: CampaignView,
    pub shift: ShiftView,
    pub orders: Vec<OrderView>,
    pub stations: Vec<StationView>,
    pub controls: Vec<&'static str>,
}

pub struct Session<'a> {
    campaign: &'a Campaign,
    started: bool,
    elapsed_ms: u64,
    score: i64,
    orders: Vec<OrderRuntime>,
}

impl<'a> Session<'a> {
    pub fn new(campaign: &'a Campaign) -> Self {
        let orders = campaign
            .shift
            .orders
            .iter()
            .map(|order| {
                let recipe = campaign.recipe(&order.recipe).expect("validated recipe");
                OrderRuntime {
                    phase: OrderPhase::Scheduled,
                    components: vec![ComponentPhase::Waiting; recipe.components.len()],
                    earned_score: 0,
                }
            })
            .collect();
        Self {
            campaign,
            started: false,
            elapsed_ms: 0,
            score: 0,
            orders,
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
            self.elapsed_ms = self.next_boundary(target_ms).max(self.elapsed_ms + 1);
            self.process_due();
        }
        Ok(())
    }

    pub fn begin(
        &mut self,
        order_id: &str,
        component_id: &str,
        station_id: &str,
    ) -> Result<(u64, u64), String> {
        self.require_running()?;
        let order_index = self.order_index(order_id)?;
        if self.orders[order_index].phase != OrderPhase::Open {
            return Err(format!("order {order_id:?} is not open"));
        }
        let (_, component_index, component) = self.component(order_index, component_id)?;
        let required_station = component.station;
        let ready_after_ms = component.ready_ms;
        let burn_after_ms = component.burn_ms;
        let station_index = self.station_index(station_id)?;
        if self.campaign.shift.stations[station_index].kind != required_station {
            return Err(format!(
                "component {component_id:?} cannot use station {station_id:?}"
            ));
        }
        if self.station_occupant(station_index).is_some() {
            return Err(format!("station {station_id:?} is occupied"));
        }
        if self.orders[order_index].components[component_index] != ComponentPhase::Waiting {
            return Err(format!("component {component_id:?} is not waiting"));
        }
        let ready_ms = self.elapsed_ms.saturating_add(ready_after_ms);
        let burn_ms = self.elapsed_ms.saturating_add(burn_after_ms);
        self.orders[order_index].components[component_index] = ComponentPhase::Cooking {
            station_index,
            ready_ms,
            burn_ms,
        };
        Ok((ready_ms, burn_ms))
    }

    pub fn finish(&mut self, order_id: &str, component_id: &str) -> Result<(), String> {
        self.require_running()?;
        let order_index = self.order_index(order_id)?;
        if self.orders[order_index].phase != OrderPhase::Open {
            return Err(format!("order {order_id:?} is not open"));
        }
        let (_, component_index, _) = self.component(order_index, component_id)?;
        match self.orders[order_index].components[component_index] {
            ComponentPhase::Cooking {
                ready_ms,
                burn_ms: _,
                ..
            } if self.elapsed_ms < ready_ms => Err(format!(
                "component {component_id:?} needs {} more ms",
                ready_ms - self.elapsed_ms
            )),
            ComponentPhase::Cooking { burn_ms, .. } if self.elapsed_ms < burn_ms => {
                self.orders[order_index].components[component_index] = ComponentPhase::Done;
                Ok(())
            }
            ComponentPhase::Burnt { .. } => Err(format!("component {component_id:?} is burnt")),
            _ => Err(format!("component {component_id:?} is not cooking")),
        }
    }

    pub fn discard(&mut self, station_id: &str) -> Result<(), String> {
        self.require_running()?;
        let station_index = self.station_index(station_id)?;
        let (order_index, component_index) = self
            .station_occupant(station_index)
            .ok_or_else(|| format!("station {station_id:?} is empty"))?;
        self.orders[order_index].components[component_index] = ComponentPhase::Waiting;
        Ok(())
    }

    pub fn assemble(&mut self, order_id: &str) -> Result<(), String> {
        self.require_running()?;
        let order_index = self.order_index(order_id)?;
        if self.orders[order_index].phase != OrderPhase::Open {
            return Err(format!("order {order_id:?} is not open"));
        }
        if !self.orders[order_index]
            .components
            .iter()
            .all(|component| *component == ComponentPhase::Done)
        {
            return Err("all components must be finished before assembly".to_owned());
        }
        self.orders[order_index].phase = OrderPhase::Assembled;
        Ok(())
    }

    pub fn serve(&mut self, order_id: &str) -> Result<i64, String> {
        self.require_running()?;
        let order_index = self.order_index(order_id)?;
        if self.orders[order_index].phase != OrderPhase::Assembled {
            return Err(format!("order {order_id:?} is not assembled"));
        }
        let definition = &self.campaign.shift.orders[order_index];
        let recipe = self
            .campaign
            .recipe(&definition.recipe)
            .expect("validated recipe");
        let deadline = definition.arrival_ms.saturating_add(definition.patience_ms);
        let earned = recipe.base_score + deadline.saturating_sub(self.elapsed_ms) as i64 / 10_000;
        self.orders[order_index].phase = OrderPhase::Served;
        self.orders[order_index].earned_score = earned;
        self.score += earned;
        Ok(earned)
    }

    pub fn snapshot(&self) -> Snapshot {
        Snapshot {
            schema: STATE_SCHEMA,
            campaign: CampaignView {
                id: self.campaign.id.clone(),
                title: self.campaign.title.clone(),
                score: self.score,
                max_score: self.campaign.max_score,
            },
            shift: ShiftView {
                id: self.campaign.shift.id.clone(),
                title: self.campaign.shift.title.clone(),
                status: if !self.started {
                    "not_started"
                } else if self.elapsed_ms >= self.duration_ms() {
                    "complete"
                } else {
                    "running"
                }
                .to_owned(),
                elapsed_ms: self.elapsed_ms,
                duration_ms: self.duration_ms(),
                remaining_ms: self.duration_ms().saturating_sub(self.elapsed_ms),
            },
            orders: self
                .campaign
                .shift
                .orders
                .iter()
                .enumerate()
                .filter(|(index, _)| self.orders[*index].phase != OrderPhase::Scheduled)
                .map(|(index, definition)| self.order_view(index, definition.id.as_str()))
                .collect(),
            stations: self
                .campaign
                .shift
                .stations
                .iter()
                .enumerate()
                .map(|(index, _)| self.station_view(index))
                .collect(),
            controls: vec![
                "start", "show", "begin", "finish", "discard", "assemble", "serve", "alarm",
                "cancel", "wait", "submit",
            ],
        }
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

    fn order_index(&self, id: &str) -> Result<usize, String> {
        self.campaign
            .shift
            .orders
            .iter()
            .position(|order| order.id == id)
            .ok_or_else(|| format!("unknown order {id:?}"))
    }

    fn station_index(&self, id: &str) -> Result<usize, String> {
        self.campaign
            .shift
            .stations
            .iter()
            .position(|station| station.id == id)
            .ok_or_else(|| format!("unknown station {id:?}"))
    }

    fn component(
        &self,
        order_index: usize,
        id: &str,
    ) -> Result<(&RecipeDefinition, usize, &ComponentDefinition), String> {
        let order = &self.campaign.shift.orders[order_index];
        let recipe = self
            .campaign
            .recipe(&order.recipe)
            .expect("validated recipe");
        let index = recipe
            .components
            .iter()
            .position(|component| component.id == id)
            .ok_or_else(|| format!("unknown component {id:?} for order {:?}", order.id))?;
        Ok((recipe, index, &recipe.components[index]))
    }

    fn station_occupant(&self, station_index: usize) -> Option<(usize, usize)> {
        self.orders
            .iter()
            .enumerate()
            .find_map(|(order_index, order)| {
                order
                    .components
                    .iter()
                    .enumerate()
                    .find(|(_, component)| {
                        matches!(
                            component,
                            ComponentPhase::Cooking { station_index: index, .. }
                                | ComponentPhase::Burnt { station_index: index }
                                if *index == station_index
                        )
                    })
                    .map(|(component_index, _)| (order_index, component_index))
            })
    }

    fn next_boundary(&self, target_ms: u64) -> u64 {
        let mut next = target_ms.min(self.duration_ms());
        for (index, definition) in self.campaign.shift.orders.iter().enumerate() {
            match self.orders[index].phase {
                OrderPhase::Scheduled if definition.arrival_ms > self.elapsed_ms => {
                    next = next.min(definition.arrival_ms)
                }
                OrderPhase::Open | OrderPhase::Assembled => {
                    let deadline = definition.arrival_ms.saturating_add(definition.patience_ms);
                    if deadline > self.elapsed_ms {
                        next = next.min(deadline);
                    }
                }
                _ => {}
            }
            for component in &self.orders[index].components {
                if let ComponentPhase::Cooking {
                    ready_ms, burn_ms, ..
                } = component
                {
                    if *ready_ms > self.elapsed_ms {
                        next = next.min(*ready_ms);
                    }
                    if *burn_ms > self.elapsed_ms {
                        next = next.min(*burn_ms);
                    }
                }
            }
        }
        next
    }

    fn process_due(&mut self) {
        for (index, definition) in self.campaign.shift.orders.iter().enumerate() {
            if self.orders[index].phase == OrderPhase::Scheduled
                && definition.arrival_ms <= self.elapsed_ms
            {
                self.orders[index].phase = OrderPhase::Open;
            }
            let deadline = definition.arrival_ms.saturating_add(definition.patience_ms);
            if matches!(
                self.orders[index].phase,
                OrderPhase::Open | OrderPhase::Assembled
            ) && deadline <= self.elapsed_ms
            {
                self.orders[index].phase = OrderPhase::Missed;
                for component in &mut self.orders[index].components {
                    if let ComponentPhase::Cooking { station_index, .. } = *component {
                        *component = ComponentPhase::Burnt { station_index };
                    }
                }
            }
            for component in &mut self.orders[index].components {
                if let ComponentPhase::Cooking {
                    station_index,
                    burn_ms,
                    ..
                } = *component
                    && burn_ms <= self.elapsed_ms
                {
                    *component = ComponentPhase::Burnt { station_index };
                }
            }
        }
        if self.elapsed_ms >= self.duration_ms() {
            for order in &mut self.orders {
                if matches!(
                    order.phase,
                    OrderPhase::Scheduled | OrderPhase::Open | OrderPhase::Assembled
                ) {
                    order.phase = OrderPhase::Missed;
                }
            }
        }
    }

    fn order_view(&self, index: usize, id: &str) -> OrderView {
        let definition = &self.campaign.shift.orders[index];
        let recipe = self
            .campaign
            .recipe(&definition.recipe)
            .expect("validated recipe");
        let deadline_ms = definition.arrival_ms.saturating_add(definition.patience_ms);
        OrderView {
            id: id.to_owned(),
            recipe: recipe.id.clone(),
            title: recipe.title.clone(),
            status: enum_name(self.orders[index].phase),
            deadline_ms,
            remaining_ms: deadline_ms.saturating_sub(self.elapsed_ms),
            earned_score: self.orders[index].earned_score,
            components: recipe
                .components
                .iter()
                .zip(&self.orders[index].components)
                .map(|(definition, phase)| self.component_view(definition, phase))
                .collect(),
        }
    }

    fn component_view(
        &self,
        definition: &ComponentDefinition,
        phase: &ComponentPhase,
    ) -> ComponentView {
        let (status, ready_in_ms, burn_in_ms) = match phase {
            ComponentPhase::Waiting => ("waiting", None, None),
            ComponentPhase::Cooking {
                ready_ms, burn_ms, ..
            } if *ready_ms <= self.elapsed_ms => (
                "ready",
                Some(0),
                Some(burn_ms.saturating_sub(self.elapsed_ms)),
            ),
            ComponentPhase::Cooking {
                ready_ms, burn_ms, ..
            } => (
                "cooking",
                Some(ready_ms.saturating_sub(self.elapsed_ms)),
                Some(burn_ms.saturating_sub(self.elapsed_ms)),
            ),
            ComponentPhase::Done => ("done", None, None),
            ComponentPhase::Burnt { .. } => ("burnt", None, None),
        };
        ComponentView {
            id: definition.id.clone(),
            label: definition.label.clone(),
            station: definition.station,
            status: status.to_owned(),
            ready_in_ms,
            burn_in_ms,
        }
    }

    fn station_view(&self, station_index: usize) -> StationView {
        let station = &self.campaign.shift.stations[station_index];
        let Some((order_index, component_index)) = self.station_occupant(station_index) else {
            return StationView {
                id: station.id.clone(),
                label: station.label.clone(),
                kind: station.kind,
                status: "idle".to_owned(),
                order: None,
                component: None,
                ready_in_ms: None,
                burn_in_ms: None,
            };
        };
        let order = &self.campaign.shift.orders[order_index];
        let recipe = self
            .campaign
            .recipe(&order.recipe)
            .expect("validated recipe");
        let component = &recipe.components[component_index];
        let view = self.component_view(
            component,
            &self.orders[order_index].components[component_index],
        );
        StationView {
            id: station.id.clone(),
            label: station.label.clone(),
            kind: station.kind,
            status: view.status,
            order: Some(order.id.clone()),
            component: Some(component.id.clone()),
            ready_in_ms: view.ready_in_ms,
            burn_in_ms: view.burn_in_ms,
        }
    }
}

fn enum_name<T: Serialize>(value: T) -> String {
    serde_json::to_value(value)
        .expect("serializable enum")
        .as_str()
        .expect("unit enum serializes to string")
        .to_owned()
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
    fn future_orders_arrive_without_agent_actions() {
        let campaign = campaign();
        let mut session = Session::new(&campaign);
        session.start().expect("start");
        assert_eq!(session.snapshot().orders.len(), 1);
        session.advance_to(180_000).expect("advance");
        assert_eq!(session.snapshot().orders.len(), 2);
    }

    #[test]
    fn component_requires_a_manual_finish_inside_its_window() {
        let campaign = campaign();
        let mut session = Session::new(&campaign);
        session.start().expect("start");
        session
            .begin("burger-1", "bun", "prep-1")
            .expect("start bun");
        assert!(session.finish("burger-1", "bun").is_err());
        session.advance_to(60_000).expect("ready");
        assert_eq!(session.snapshot().stations[0].status, "ready");
        session.finish("burger-1", "bun").expect("finish bun");
        assert_eq!(session.snapshot().stations[0].status, "idle");
    }

    #[test]
    fn burnt_food_blocks_the_station_until_discarded() {
        let campaign = campaign();
        let mut session = Session::new(&campaign);
        session.start().expect("start");
        session
            .begin("burger-1", "patty", "griddle-1")
            .expect("start patty");
        session.advance_to(240_000).expect("burn patty");
        assert_eq!(session.snapshot().stations[1].status, "burnt");
        assert!(session.begin("burger-2", "patty", "griddle-1").is_err());
        session.discard("griddle-1").expect("discard");
        assert_eq!(session.snapshot().stations[1].status, "idle");
    }

    #[test]
    fn a_completed_order_scores_remaining_patience() {
        let campaign = campaign();
        let mut session = Session::new(&campaign);
        session.start().expect("start");
        session
            .begin("burger-1", "bun", "prep-1")
            .expect("start bun");
        session
            .begin("burger-1", "patty", "griddle-1")
            .expect("start patty");
        session.advance_to(60_000).expect("bun ready");
        session.finish("burger-1", "bun").expect("finish bun");
        session.advance_to(180_000).expect("patty ready");
        session.finish("burger-1", "patty").expect("finish patty");
        session.assemble("burger-1").expect("assemble");
        assert_eq!(session.serve("burger-1").expect("serve"), 142);
        assert_eq!(session.snapshot().campaign.score, 142);
    }
}
