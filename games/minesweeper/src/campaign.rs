use std::collections::HashSet;
use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Campaign {
    pub schema: String,
    pub id: String,
    pub title: String,
    pub generator: String,
    pub solver: String,
    pub difficulty_metric: String,
    pub distribution_metric: String,
    pub tiers: Vec<Tier>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Tier {
    pub id: String,
    pub title: String,
    pub difficulty: String,
    pub unlock_after: usize,
    pub opening_distribution: OpeningDistributionProfile,
    pub levels: Vec<Level>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct OpeningDistributionProfile {
    pub min_weighted_mean_percent: usize,
    pub max_weighted_mean_percent: usize,
    pub min_upper_median_percent: usize,
    pub max_upper_median_percent: usize,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Level {
    pub id: String,
    pub title: String,
    pub width: usize,
    pub height: usize,
    pub mines: usize,
    pub seed: u64,
    pub proof: ProofProfile,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProofProfile {
    pub min_opening_percent: usize,
    pub max_opening_percent: usize,
    pub min_proof_rounds: usize,
    pub min_subset_rounds: usize,
    pub min_frontier: usize,
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
        if self.schema != "minesweeper-campaign-v3"
            || self.generator != "first-click-difficulty-v1"
            || self.solver != "local-subset-v1"
            || self.difficulty_metric != "proof-profile-v1"
            || self.distribution_metric != "tier-opening-distribution-v1"
        {
            return Err(
                "unsupported campaign, generator, solver, or difficulty version".to_owned(),
            );
        }
        if self.id.trim().is_empty() || self.tiers.is_empty() {
            return Err("campaign id and tiers must not be empty".to_owned());
        }
        let mut tier_ids = HashSet::new();
        let mut level_ids = HashSet::new();
        for (tier_index, tier) in self.tiers.iter().enumerate() {
            if !tier_ids.insert(&tier.id) || tier.levels.is_empty() {
                return Err(format!("tier {} is empty or duplicated", tier.id));
            }
            if (tier_index == 0 && tier.unlock_after != 0)
                || (tier_index > 0
                    && tier.unlock_after != self.tiers[tier_index - 1].levels.len().div_ceil(2))
            {
                return Err(format!("tier {} has an invalid unlock threshold", tier.id));
            }
            let distribution = &tier.opening_distribution;
            if !(1..=100).contains(&distribution.min_weighted_mean_percent)
                || !(1..=100).contains(&distribution.max_weighted_mean_percent)
                || distribution.min_weighted_mean_percent > distribution.max_weighted_mean_percent
                || !(1..=100).contains(&distribution.min_upper_median_percent)
                || !(1..=100).contains(&distribution.max_upper_median_percent)
                || distribution.min_upper_median_percent > distribution.max_upper_median_percent
            {
                return Err(format!(
                    "tier {} has an invalid opening distribution profile",
                    tier.id
                ));
            }
            if tier_index > 0 {
                let previous = &self.tiers[tier_index - 1].opening_distribution;
                if distribution.max_weighted_mean_percent >= previous.min_weighted_mean_percent
                    || distribution.max_upper_median_percent >= previous.min_upper_median_percent
                {
                    return Err(format!(
                        "tier {} does not preserve the opening-distribution difficulty order",
                        tier.id
                    ));
                }
            }
            for level in &tier.levels {
                if !level_ids.insert(&level.id) {
                    return Err(format!("duplicate level id: {}", level.id));
                }
                if !(5..=16).contains(&level.width)
                    || !(5..=16).contains(&level.height)
                    || level.mines == 0
                    || level.mines + 9 > level.width * level.height
                    || !(1..=100).contains(&level.proof.min_opening_percent)
                    || !(1..=100).contains(&level.proof.max_opening_percent)
                    || level.proof.min_opening_percent > level.proof.max_opening_percent
                    || level.proof.min_subset_rounds > level.proof.min_proof_rounds
                    || level.proof.min_frontier > level.width * level.height - level.mines
                {
                    return Err(format!(
                        "level {} has invalid dimensions, mine count, or proof profile",
                        level.id
                    ));
                }
            }
            let minimum_opening = tier
                .levels
                .iter()
                .map(|level| level.proof.min_opening_percent)
                .min()
                .expect("non-empty tier");
            let maximum_opening = tier
                .levels
                .iter()
                .map(|level| level.proof.max_opening_percent)
                .max()
                .expect("non-empty tier");
            if distribution.min_weighted_mean_percent < minimum_opening
                || distribution.max_weighted_mean_percent > maximum_opening
                || distribution.min_upper_median_percent < minimum_opening
                || distribution.max_upper_median_percent > maximum_opening
            {
                return Err(format!(
                    "tier {} has a distribution band outside its per-board opening bands",
                    tier.id
                ));
            }
        }
        Ok(())
    }

    pub fn max_score(&self) -> usize {
        self.tiers.len()
    }

    pub fn wins_required(&self, tier_index: usize) -> usize {
        self.tiers.get(tier_index + 1).map_or_else(
            || self.tiers[tier_index].levels.len().div_ceil(2),
            |tier| tier.unlock_after,
        )
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frozen_campaign_is_valid_and_progressive() {
        let campaign = Campaign::load(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("data/campaign/minesweeper.json"),
        )
        .expect("campaign");
        assert_eq!(campaign.tiers.len(), 5);
        assert_eq!(
            campaign
                .tiers
                .iter()
                .map(|tier| tier.levels.len())
                .collect::<Vec<_>>(),
            [10, 10, 10, 10, 10]
        );
        assert_eq!(campaign.max_score(), 5);
        assert_eq!(
            (0..campaign.tiers.len())
                .map(|index| campaign.wins_required(index))
                .collect::<Vec<_>>(),
            [5, 5, 5, 5, 5]
        );
    }
}
