//! Strategy: an opener chosen at game start, the final build order once the
//! enemy race is scouted.

use crate::util::Rng;
use bwapi::Race;

#[derive(Clone, Copy)]
pub enum Opener {
    Pool4,
    Standard,
}

#[derive(Clone, Copy, PartialEq)]
pub enum BuildOrder {
    Pool4,
    Speedling,
    HydraRush,
    MutaRush,
}

impl BuildOrder {
    pub fn name(self) -> &'static str {
        match self {
            BuildOrder::Pool4 => "4pool",
            BuildOrder::Speedling => "speedling",
            BuildOrder::HydraRush => "hydras",
            BuildOrder::MutaRush => "mutas",
        }
    }

    /// The intended tech chain; one log line at scouted makes the log
    /// self-explanatory when a piece is missing later.
    pub fn tech_line(self) -> &'static str {
        match self {
            BuildOrder::Pool4 => "lings only, no tech",
            BuildOrder::Speedling => "lings, then lair for speed upgrade",
            BuildOrder::HydraRush => "lair, den, hydras",
            BuildOrder::MutaRush => "lair, spire, mutas, nest, hive, cavern, ultras",
        }
    }
}

pub struct Strategy {
    rng: Rng,
    pub opener: Opener,
    /// `None` until the enemy race is scouted; a `Pool4` opener is final at once.
    pub order: Option<BuildOrder>,
    /// Sunken count; race-adjusted when the order is finalized.
    pub num_sunkens: i32,
}

impl Strategy {
    /// Debug: always the standard opener until the randomized pipeline is
    /// verified; flip back to the weighted pick afterwards.
    pub fn new(seed: u64) -> Self {
        let rng = Rng::new(seed);
        let opener = Opener::Standard;
        let order = match opener {
            Opener::Pool4 => Some(BuildOrder::Pool4),
            Opener::Standard => None,
        };
        Strategy { rng, opener, order, num_sunkens: 2 }
    }

    /// Picks the final order by enemy race.
    pub fn finalize(&mut self, enemy_race: Race) {
        if self.order.is_some() {
            return;
        }
        let order = match enemy_race {
            Race::ZERG => self.rng.pick(&[(BuildOrder::MutaRush, 3), (BuildOrder::Speedling, 1)]),
            Race::TERRAN => self.rng.pick(&[(BuildOrder::HydraRush, 3), (BuildOrder::MutaRush, 2)]),
            Race::PROTOSS => self.rng.pick(&[(BuildOrder::MutaRush, 2), (BuildOrder::HydraRush, 2)]),
            _ => BuildOrder::MutaRush,
        };
        self.num_sunkens = if enemy_race == Race::ZERG { 3 } else { 2 };
        self.order = Some(order);
    }
}
