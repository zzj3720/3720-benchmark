use std::collections::HashSet;
use std::fs;
use std::path::Path;

use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Campaign {
    pub schema: String,
    pub id: String,
    pub title: String,
    pub tiers: Vec<Tier>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Tier {
    pub id: String,
    pub title: String,
    pub difficulty: String,
    pub source_url: String,
    pub unlock_after: usize,
    pub levels: Vec<Level>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Level {
    pub id: String,
    pub source_id: String,
    pub title: String,
    pub width: usize,
    pub height: usize,
    pub rows: Vec<String>,
}

impl Campaign {
    pub fn load(path: impl AsRef<Path>) -> Result<Self, String> {
        let path = path.as_ref();
        let bytes = fs::read(path)
            .map_err(|error| format!("could not read {}: {error}", path.display()))?;
        let campaign: Self = serde_json::from_slice(&bytes)
            .map_err(|error| format!("invalid campaign {}: {error}", path.display()))?;
        campaign.validate()?;
        Ok(campaign)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != "sokoban-campaign-v1" {
            return Err(format!("unsupported campaign schema: {}", self.schema));
        }
        if self.id.trim().is_empty() || self.tiers.is_empty() {
            return Err("campaign id and tiers must not be empty".to_owned());
        }
        let mut tier_ids = HashSet::new();
        let mut level_ids = HashSet::new();
        for tier in &self.tiers {
            if !tier_ids.insert(&tier.id) {
                return Err(format!("duplicate tier id: {}", tier.id));
            }
            if tier.levels.is_empty() {
                return Err(format!("tier {} has no levels", tier.id));
            }
            let expected_unlock = tier.levels.len().div_ceil(2);
            if tier.unlock_after != expected_unlock {
                return Err(format!(
                    "tier {} unlock_after must be ceil(levels / 2), expected {expected_unlock}",
                    tier.id
                ));
            }
            for level in &tier.levels {
                if !level_ids.insert(&level.id) {
                    return Err(format!("duplicate level id: {}", level.id));
                }
                validate_level(level)?;
            }
        }
        Ok(())
    }

    pub fn max_score(&self) -> usize {
        self.tiers.iter().map(|tier| tier.levels.len()).sum()
    }

    pub fn find_level(&self, id: &str) -> Option<(usize, usize, &Level)> {
        self.tiers
            .iter()
            .enumerate()
            .find_map(|(tier_index, tier)| {
                tier.levels
                    .iter()
                    .enumerate()
                    .find(|(_, level)| level.id == id)
                    .map(|(level_index, level)| (tier_index, level_index, level))
            })
    }
}

fn validate_level(level: &Level) -> Result<(), String> {
    if level.width == 0 || level.height == 0 || level.rows.len() != level.height {
        return Err(format!("level {} has invalid dimensions", level.id));
    }
    let mut players = 0;
    let mut boxes = 0;
    let mut goals = 0;
    for row in &level.rows {
        if row.chars().count() != level.width {
            return Err(format!("level {} has a row with the wrong width", level.id));
        }
        for tile in row.chars() {
            match tile {
                '#' | ' ' => {}
                '@' => players += 1,
                '$' => boxes += 1,
                '.' => goals += 1,
                '*' => {
                    boxes += 1;
                    goals += 1;
                }
                '+' => {
                    players += 1;
                    goals += 1;
                }
                _ => {
                    return Err(format!(
                        "level {} contains unsupported tile {tile:?}",
                        level.id
                    ));
                }
            }
        }
    }
    if players != 1 || boxes == 0 || boxes != goals {
        return Err(format!(
            "level {} must contain one player and equal non-zero box/goal counts (players={players}, boxes={boxes}, goals={goals})",
            level.id
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frozen_campaign_is_valid_and_progressive() {
        let campaign = Campaign::load(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("data/campaign/sokoban.json"),
        )
        .expect("campaign");
        assert_eq!(campaign.tiers.len(), 4);
        assert_eq!(
            campaign
                .tiers
                .iter()
                .map(|tier| tier.levels.len())
                .collect::<Vec<_>>(),
            [50, 155, 50, 50]
        );
        assert_eq!(campaign.max_score(), 305);
        assert_eq!(
            campaign
                .tiers
                .iter()
                .map(|tier| tier.unlock_after)
                .collect::<Vec<_>>(),
            [25, 78, 25, 25]
        );
    }
}
