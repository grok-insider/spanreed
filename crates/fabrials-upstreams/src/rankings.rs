//! Provider plan orders for the provider-neutral autosteer in
//! `fabrials-accounts`.

use fabrials_accounts::{PlanRanks, RankTable};

/// SuperGrok and X plans, best first.
pub const GROK_PLANS: RankTable = RankTable::new(&[
    ("heavy", 5),
    ("plus", 4),
    ("normal", 3),
    ("lite", 2),
    ("premium-plus", 1),
    ("premium", 0),
]);

/// Grok burn order: smaller SuperGrok / X pools first; Heavy is reserve.
pub const GROK_BURN: RankTable = GROK_PLANS.inverted();

/// Cursor plans, higher tier first.
pub const CURSOR_PLANS: RankTable = RankTable::new(&[
    ("business", 4),
    ("enterprise", 4),
    ("ultra", 4),
    ("pro", 3),
    ("pro-plus", 3),
    ("plus", 2),
    ("hobby", 1),
    ("free", 1),
]);

/// The default registry: Grok burns small pools first, Cursor-backed routes
/// prefer the higher tier.
pub fn default_rankings() -> PlanRanks {
    PlanRanks::new()
        .with("grok", GROK_BURN)
        .with("cursor", CURSOR_PLANS)
        .with("grok-bot", CURSOR_PLANS)
}

#[cfg(test)]
mod tests {
    use super::*;
    use fabrials_accounts::PlanRanking;

    #[test]
    fn tables_keep_the_previous_ranks() {
        for (slug, plan, burn) in [
            ("heavy", 5, 0),
            ("plus", 4, 1),
            ("normal", 3, 2),
            ("lite", 2, 3),
            ("premium-plus", 1, 4),
            ("premium", 0, 5),
            ("unknown", 0, 5),
        ] {
            assert_eq!(GROK_PLANS.rank(slug), plan, "{slug}");
            assert_eq!(GROK_BURN.rank(slug), burn, "{slug}");
        }
        for (slug, rank) in [
            ("enterprise", 4),
            ("pro-plus", 3),
            ("plus", 2),
            ("free", 1),
            ("other", 0),
        ] {
            assert_eq!(CURSOR_PLANS.rank(slug), rank, "{slug}");
        }
        let ranks = default_rankings();
        assert_eq!(ranks.rank("grok", "heavy"), 0);
        assert_eq!(ranks.rank("grok-bot", "ultra"), 4);
    }
}
