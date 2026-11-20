//! Worker and depot orders: demo mining until the full gathering discipline.

use crate::army;
use crate::build_order;
use crate::strategy::BuildOrder;
use crate::ZzzkBot;
use bwapi::{Context, CoordinateType, UnitType};

/// Maximum distance from an owned resource depot to a mineral patch (pixels).
const MINERAL_DEPOT_RADIUS: i32 = 10 * 32;

/// Drones on one geyser.
const GAS_WORKERS: usize = 3;

/// The macro layer: issues build orders by the current strategy.
pub fn macro_orders<'game>(cx: Context<'_, 'game>, bot: &mut ZzzkBot<'game>) {
    let game = cx.game();
    let counts = crate::util::own_counts(&bot.own_units, cx);
    let have_pool =
        counts.all.get(&UnitType::ZERG_SPAWNING_POOL).copied().unwrap_or(0) > 0;
    if !have_pool {
        let pool = UnitType::ZERG_SPAWNING_POOL;
        let status = match bot.build.plan(cx, &bot.own_units, &bot.scouting, pool) {
            Some(plan) => {
                // Pool is the top priority: the builder goes right now.
                if !plan.builder.build(plan.unit_type, plan.tile) {
                    bot.build.mark_fail(pool, game.frame_count());
                }
                format!("pool: builder #{}, loc {},{}", plan.builder.id(), plan.tile.x, plan.tile.y)
            }
            None => String::from("pool: waiting (minerals, throttle, builder or loc)"),
        };
        game.draw_text(CoordinateType::Screen, 10, 74, &status);
        return;
    }

    // While the pool is still to start, its 200 minerals are the budget: an
    // overlord morph here would eat them mid-flight of the builder.
    let me = game.self_player().expect("the bot plays a match, not a replay");
    let free = me.supply_total() - counts.supply_used;
    build_order::extractor_trick(cx, bot, free);
    army::supply_overlords(cx, bot, &counts);
    let made = army::morph_lings(cx, bot);

    manage_gas(cx, bot);
    manage_hatcheries(cx, bot);
    manage_lair(cx, bot);
    manage_tier3(cx, bot);
    let sunkens = manage_sunkens(cx, bot);
    let extra = if made > 0 {
        format!(", lings +{made}")
    } else {
        String::new()
    };
    let hatch_count = counts
        .all
        .get(&UnitType::ZERG_HATCHERY)
        .copied()
        .unwrap_or(0)
        + counts.all.get(&UnitType::ZERG_LAIR).copied().unwrap_or(0)
        + counts.all.get(&UnitType::ZERG_HIVE).copied().unwrap_or(0);
    game.draw_text(
        CoordinateType::Screen,
        10,
        74,
        &format!(
            "pool: built{extra}, gas workers {}, hatch {hatch_count}, sunkens {}",
            bot.gas_drones.len(),
            sunkens,
        ),
    );
}

/// Second hatchery for non-all-in orders: one is not enough larva flow.
fn manage_hatcheries<'game>(cx: Context<'_, 'game>, bot: &mut ZzzkBot<'game>) {
    if bot.strategy.order == Some(BuildOrder::Pool4) {
        return; // all-in: minerals go into zerglings, not bases
    }
    let counts = crate::util::own_counts(&bot.own_units, cx);
    let bases = counts
        .all
        .get(&UnitType::ZERG_HATCHERY)
        .copied()
        .unwrap_or(0)
        + counts.all.get(&UnitType::ZERG_LAIR).copied().unwrap_or(0)
        + counts.all.get(&UnitType::ZERG_HIVE).copied().unwrap_or(0);
    if bases >= 2 {
        return;
    }
    let hatchery = UnitType::ZERG_HATCHERY;
    if let Some(plan) = bot.build.plan(cx, &bot.own_units, &bot.scouting, hatchery) {
        if !plan.builder.build(plan.unit_type, plan.tile) {
            bot.build.mark_fail(hatchery, cx.game().frame_count());
        }
    }
}

/// Base defense: creep colonies up to the strategy's sunken count, finished
/// ones morphed into sunken colonies. Returns that count, colonies included.

/// Morphs the first hatchery into a lair once the pool and gas are up and
/// the order wants tier-2 tech (mutas and hydras both need it).
fn manage_lair<'game>(cx: Context<'_, 'game>, bot: &mut ZzzkBot<'game>) {
    let wants_t2 = matches!(
        bot.strategy.order,
        Some(BuildOrder::MutaRush) | Some(BuildOrder::HydraRush)
    );
    if !wants_t2 {
        return;
    }
    let counts = crate::util::own_counts(&bot.own_units, cx);
    let pool_done =
        counts.completed.get(&UnitType::ZERG_SPAWNING_POOL).copied().unwrap_or(0) > 0;
    let gas = counts.completed.get(&UnitType::ZERG_EXTRACTOR).copied().unwrap_or(0) > 0;
    let tier2 = counts
        .all
        .get(&UnitType::ZERG_LAIR)
        .copied()
        .unwrap_or(0)
        + counts.all.get(&UnitType::ZERG_HIVE).copied().unwrap_or(0)
        + counts
            .all
            .get(&UnitType::ZERG_QUEENS_NEST)
            .copied()
            .unwrap_or(0);
    if !pool_done || !gas || tier2 > 0 {
        return;
    }
    let game = cx.game();
    if let Some(hatchery) = bot.own_units.iter(cx).find(|u| {
        u.exists() && u.is_completed() && !u.is_morphing()
            && u.unit_type() == UnitType::ZERG_HATCHERY
    }) {
        if !hatchery.morph(UnitType::ZERG_LAIR) {
            game.send_text(&format!("lair morph failed: {:?}", game.last_error()));
        }
    }
}

/// Tier-3 chain for the muta order: a drone-built queen's nest once the lair
/// is up, then a hive when the nest completes. Hive itself enables nothing
/// yet — ultra cavern is a later step — but the morph takes long, so it
/// starts early.
fn manage_tier3<'game>(cx: Context<'_, 'game>, bot: &mut ZzzkBot<'game>) {
    if bot.strategy.order != Some(BuildOrder::MutaRush) {
        return;
    }
    let game = cx.game();
    let counts = crate::util::own_counts(&bot.own_units, cx);
    let lair_done = counts.completed.get(&UnitType::ZERG_LAIR).copied().unwrap_or(0) > 0;
    let nests = counts
        .all
        .get(&UnitType::ZERG_QUEENS_NEST)
        .copied()
        .unwrap_or(0);
    let hives = counts.all.get(&UnitType::ZERG_HIVE).copied().unwrap_or(0);

    // The nest is a drone building (whatBuilds = Drone, Lair is a
    // prerequisite only); morphing a lair would silently fail.
    if lair_done && nests == 0 {
        let nest = UnitType::ZERG_QUEENS_NEST;
        if let Some(plan) = bot.build.plan(cx, &bot.own_units, &bot.scouting, nest) {
            if !plan.builder.build(plan.unit_type, plan.tile) {
                bot.build.mark_fail(nest, game.frame_count());
            }
        }
        return;
    }
    if nests > 0 && hives == 0 {
        let nest_done =
            counts.completed.get(&UnitType::ZERG_QUEENS_NEST).copied().unwrap_or(0) > 0;
        if nest_done {
            if let Some(nest) = bot.own_units.iter(cx).find(|u| {
                u.exists() && u.is_completed() && !u.is_morphing()
                    && u.unit_type() == UnitType::ZERG_QUEENS_NEST
            }) {
                if !nest.morph(UnitType::ZERG_HIVE) {
                    game.send_text(&format!("hive morph failed: {:?}", game.last_error()));
                }
            }
        }
    }
}

/// Base defense: creep colonies up to the strategy's sunken count, finished
/// ones morphed into sunken colonies. Returns that count, colonies included.
fn manage_sunkens<'game>(cx: Context<'_, 'game>, bot: &mut ZzzkBot<'game>) -> i32 {
    let game = cx.game();
    let want = bot.strategy.num_sunkens;

    let mut have: i32 = 0;
    let mut idle_colonies: Vec<_> = Vec::new();
    for unit in bot.own_units.iter(cx) {
        if !unit.exists() {
            continue;
        }
        let unit_type = unit.unit_type();
        if unit_type == UnitType::ZERG_SUNKEN_COLONY
            || unit.build_type() == UnitType::ZERG_SUNKEN_COLONY
            || unit.build_type() == UnitType::ZERG_CREEP_COLONY
        {
            have += 1;
        }
        if unit_type == UnitType::ZERG_CREEP_COLONY && unit.is_completed() {
            idle_colonies.push(unit);
        }
    }

    // The morph needs a finished pool; colonies just need creep.
    let counts = crate::util::own_counts(&bot.own_units, cx);
    let pool_done =
        counts.completed.get(&UnitType::ZERG_SPAWNING_POOL).copied().unwrap_or(0) > 0;
    if pool_done {
        for colony in idle_colonies {
            if have >= want {
                break;
            }
            if colony.morph(UnitType::ZERG_SUNKEN_COLONY) {
                have += 1;
            }
        }
    }

    if have < want {
        let colony = UnitType::ZERG_CREEP_COLONY;
        if let Some(plan) = bot.build.plan(cx, &bot.own_units, &bot.scouting, colony) {
            if !plan.builder.build(plan.unit_type, plan.tile) {
                bot.build.mark_fail(colony, game.frame_count());
            }
        }
    }
    have
}

/// Builds the extractor after the pool (not on a 4-pool all-in) and staffs
/// it with three drones once completed.
fn manage_gas<'game>(cx: Context<'_, 'game>, bot: &mut ZzzkBot<'game>) {
    let game = cx.game();
    // Dead drones and stale refinery leave the roster each frame.
    bot.gas_drones.retain(|d| d.exists());
    bot.refinery = bot
        .own_units
        .iter(cx)
        .find(|u| u.exists() && u.is_completed() && u.unit_type().is_refinery());

    let all_in = bot.strategy.order == Some(BuildOrder::Pool4);
    let counts = crate::util::own_counts(&bot.own_units, cx);
    let refineries = counts.all.get(&UnitType::ZERG_EXTRACTOR).copied().unwrap_or(0);
    if !all_in && refineries == 0 {
        let extractor = UnitType::ZERG_EXTRACTOR;
        if let Some(plan) = bot.build.plan(cx, &bot.own_units, &bot.scouting, extractor) {
            if !plan.builder.build(plan.unit_type, plan.tile) {
                bot.build.mark_fail(extractor, game.frame_count());
            }
        }
    }

    // Staff the completed refinery: reassign the drones nearest to it.
    let Some(refinery) = bot.refinery else { return };
    while bot.gas_drones.len() < GAS_WORKERS {
        let Some(worker) = bot
            .own_units
            .iter(cx)
            .filter(|u| {
                u.exists() && u.is_completed() && !u.is_morphing()
                    && u.unit_type() == UnitType::ZERG_DRONE
                    && !bot.scouting.is_scout(*u)
                    && !bot.build.is_builder(*u)
                    && !bot.gas_drones.contains(u)
            })
            .min_by_key(|u| u.distance_to_unit(refinery))
        else {
            break;
        };
        if worker.gather(refinery) {
            bot.gas_drones.push(worker);
        } else {
            break;
        }
    }
}

impl<'game> ZzzkBot<'game> {
    /// Idle workers mine minerals near completed owned depots; depots train workers.
    pub fn give_orders(&self, cx: Context<'_, 'game>) {
        for unit in self.own_units.iter(cx) {
            let unit_type = unit.unit_type();
            if unit_type.is_worker() || self.build.is_builder(unit) {
                if !unit.is_idle() {
                    continue;
                }
                // Gas drones cycle between the refinery and the depot.
                if self.gas_drones.contains(&unit) {
                    if unit.is_carrying_gas() {
                        unit.return_cargo();
                    } else if let Some(refinery) = self.refinery {
                        unit.gather(refinery);
                    }
                    continue;
                }
                if unit.is_carrying_gas() || unit.is_carrying_minerals() {
                    unit.return_cargo();
                    continue;
                }
                // Only use minerals near an existing, completed owned depot.
                // A scout returning from the map must not start mining at an
                // unoccupied expansion merely because those minerals are closer.
                let closest = self
                    .minerals
                    .iter(cx)
                    .filter(|&mineral| {
                        mineral.exists()
                            && self.own_units.iter(cx).any(|depot| {
                                depot.exists()
                                    && depot.is_completed()
                                    && depot.unit_type().is_resource_depot()
                                    && depot.distance_to_unit(mineral) < MINERAL_DEPOT_RADIUS
                            })
                    })
                    .min_by_key(|&mineral| unit.distance_to_unit(mineral));
                if let Some(mineral) = closest {
                    unit.right_click_unit(mineral);
                }
            } else if unit_type.is_resource_depot() && !unit.is_training() {
                unit.train(unit_type.race().worker());
            }
        }
    }
}
