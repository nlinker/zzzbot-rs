//! Counters, conversions, and the RNG shared by the bot's modules.

use std::collections::HashMap;

use bwapi::{Context, UnitSet, UnitType};

/// Own units per type for one frame; eggs count as their contents, not just
/// the shell. `supply_used` follows the same counting.
#[derive(Default)]
pub struct OwnCounts {
    pub all: HashMap<UnitType, i32>,
    pub completed: HashMap<UnitType, i32>,
    pub incomplete: HashMap<UnitType, i32>,
    pub supply_used: i32,
}

pub fn own_counts<'game>(own: &UnitSet<'game>, cx: Context<'_, 'game>) -> OwnCounts {
    let mut counts = OwnCounts::default();
    for unit in own.iter(cx) {
        if !unit.exists() {
            continue;
        }
        let unit_type = unit.unit_type();
        bump(&mut counts.all, unit_type, 1);
        if unit.is_completed() {
            bump(&mut counts.completed, unit_type, 1);
        } else {
            bump(&mut counts.incomplete, unit_type, 1);
        }
        counts.supply_used += unit_type.supply_required();

        if egg_check(unit_type) {
            // Morph targets hide inside: count the contents, not just the shell.
            let build_type = unit.build_type();
            if build_type.is_valid() {
                let n = if build_type.is_two_units_in_one_egg() { 2 } else { 1 };
                bump(&mut counts.all, build_type, n);
                bump(&mut counts.incomplete, build_type, n);
                counts.supply_used += build_type.supply_required() * n;
            }
        }
    }
    counts
}

/// Is this an egg-type unit: a morphing shell whose `build_type` is the payload.
fn egg_check(unit_type: UnitType) -> bool {
    matches!(
        unit_type,
        UnitType::ZERG_EGG | UnitType::ZERG_LURKER_EGG | UnitType::ZERG_COCOON
    )
}

fn bump(map: &mut HashMap<UnitType, i32>, unit_type: UnitType, by: i32) {
    *map.entry(unit_type).or_default() += by;
}

/// BWAPI supply units are doubled (a zergling is 1); show halves like the game does.
pub fn supply_str(supply: i32) -> String {
    if supply % 2 == 0 {
        format!("{}", supply / 2)
    } else {
        format!("{}.5", supply / 2)
    }
}

/// xorshift64* RNG: enough for strategy tie-breaks, no external crates.
pub struct Rng(u64);

impl Rng {
    /// Any seed; forced odd so the state never locks on zero.
    pub fn new(seed: u64) -> Self {
        Rng(seed | 1)
    }

    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// Weighted pick; weights must not all be zero.
    pub fn pick<'a, T: Copy>(&mut self, choices: &'a [(T, u32)]) -> T {
        let total: u32 = choices.iter().map(|&(_, w)| w).sum();
        let mut roll = (self.next_u64() % total as u64) as u32;
        for &(item, w) in choices {
            if roll < w {
                return item;
            }
            roll -= w;
        }
        choices[choices.len() - 1].0
    }
}
