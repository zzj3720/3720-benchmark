use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

pub const CAMPAIGN_SCHEMA: &str = "kitchen-campaign-v1";

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StationKind {
    Prep,
    Griddle,
    Fryer,
    Stove,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct StationDefinition {
    pub id: String,
    pub label: String,
    pub kind: StationKind,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ComponentDefinition {
    pub id: String,
    pub label: String,
    pub station: StationKind,
    pub ready_ms: u64,
    pub burn_ms: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RecipeDefinition {
    pub id: String,
    pub title: String,
    pub base_score: i64,
    pub components: Vec<ComponentDefinition>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct OrderDefinition {
    pub id: String,
    pub recipe: String,
    pub arrival_ms: u64,
    pub patience_ms: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ShiftDefinition {
    pub id: String,
    pub title: String,
    pub duration_ms: u64,
    pub stations: Vec<StationDefinition>,
    pub recipes: Vec<RecipeDefinition>,
    pub orders: Vec<OrderDefinition>,
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
        if self.shift.stations.is_empty()
            || self.shift.recipes.is_empty()
            || self.shift.orders.is_empty()
        {
            return Err("stations, recipes, and orders are required".to_owned());
        }

        let mut station_ids = HashSet::new();
        let mut station_kinds = HashSet::new();
        for station in &self.shift.stations {
            if station.id.is_empty() || !station_ids.insert(&station.id) {
                return Err(format!("invalid or duplicate station id {:?}", station.id));
            }
            station_kinds.insert(station.kind);
        }

        let mut recipes: HashMap<&str, &RecipeDefinition> = HashMap::new();
        for recipe in &self.shift.recipes {
            if recipe.id.is_empty() || recipes.insert(recipe.id.as_str(), recipe).is_some() {
                return Err(format!("invalid or duplicate recipe id {:?}", recipe.id));
            }
            if recipe.base_score < 0 || recipe.components.is_empty() {
                return Err(format!(
                    "recipe {:?} has invalid scoring or components",
                    recipe.id
                ));
            }
            let mut components = HashSet::new();
            for component in &recipe.components {
                if component.id.is_empty() || !components.insert(&component.id) {
                    return Err(format!("recipe {:?} has duplicate components", recipe.id));
                }
                if component.ready_ms == 0
                    || component.burn_ms <= component.ready_ms
                    || !station_kinds.contains(&component.station)
                {
                    return Err(format!(
                        "recipe {:?} has invalid component timing",
                        recipe.id
                    ));
                }
            }
        }

        let mut order_ids = HashSet::new();
        let mut theoretical_max = 0;
        for order in &self.shift.orders {
            if order.id.is_empty() || !order_ids.insert(&order.id) {
                return Err(format!("invalid or duplicate order id {:?}", order.id));
            }
            let recipe = recipes
                .get(order.recipe.as_str())
                .ok_or_else(|| format!("order {:?} uses an unknown recipe", order.id))?;
            if order.patience_ms == 0
                || order.arrival_ms >= self.shift.duration_ms
                || order.arrival_ms.saturating_add(order.patience_ms) > self.shift.duration_ms
            {
                return Err(format!("order {:?} has invalid timing", order.id));
            }
            theoretical_max += recipe.base_score + (order.patience_ms / 10_000) as i64;
        }
        if self.max_score != theoretical_max {
            return Err(format!(
                "max_score {} does not match theoretical maximum {theoretical_max}",
                self.max_score
            ));
        }
        Ok(())
    }

    pub fn recipe(&self, id: &str) -> Option<&RecipeDefinition> {
        self.shift.recipes.iter().find(|recipe| recipe.id == id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pilot_is_valid_and_has_three_distinct_recipes() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let campaign =
            Campaign::load(root.join("data/campaign/pilot.json")).expect("pilot campaign");
        assert_eq!(campaign.shift.recipes.len(), 3);
        assert_eq!(campaign.shift.orders.len(), 4);
        assert_eq!(campaign.max_score, 708);
    }
}
