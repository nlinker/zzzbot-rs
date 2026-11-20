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

/// Issues a planned build and logs the decision either way: builder and tile
/// on success, the BWAPI error on failure.
fn issue_build<'game>(
    cx: Context<'_, 'game>,
    bot: &mut ZzzkBot<'game>,
    plan: build_order::Plan<'game>,
    label: &str,
) {
    let game = cx.game();
    let at = format!("{} at {},{}", label, plan.tile.x, plan.tile.y);
    if plan.builder.build(plan.unit_type, plan.tile) {
        bot.log(cx, &format!("{at}: builder #{}", plan.builder.id()));
    } else {
        let error = game.last_error().name();
        bot.build.mark_fail(plan.unit_type, game.frame_count());
        bot.log(cx, &format!("{at}: failed: {error}"));
    }
}

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
                issue_build(cx, bot, plan, "pool");
                String::from("pool: ordered")
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
    army::morph_mutas(cx, bot);
    army::morph_hydras(cx, bot);
    army::morph_ultras(cx, bot);

    manage_gas(cx, bot);
    manage_hatcheries(cx, bot);
    manage_lair(cx, bot);
    manage_tier3(cx, bot);
    manage_spire(cx, bot);
    manage_den(cx, bot);
    manage_cavern(cx, bot);
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
    // Diagnostic: build-engine state per pending type (drop once pipeline is stable).
    let dbg = format!(
        "dbg nest {} colony {} hatch {} | m {} g {}",
        bot.build.debug_line(UnitType::ZERG_QUEENS_NEST, game.frame_count()),
        bot.build.debug_line(UnitType::ZERG_CREEP_COLONY, game.frame_count()),
        bot.build.debug_line(UnitType::ZERG_HATCHERY, game.frame_count()),
        me.minerals(),
        me.gas(),
    );
    game.draw_text(CoordinateType::Screen, 10, 92, &dbg);
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
        issue_build(cx, bot, plan, "hatchery");
    }
}

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
    if !bot.build.can_retry(UnitType::ZERG_LAIR, game.frame_count()) {
        return;
    }
    let me = game.self_player().expect("the bot plays a match, not a replay");
    if me.minerals() < UnitType::ZERG_LAIR.mineral_price() {
        return;
    }
    if let Some(hatchery) = bot.own_units.iter(cx).find(|u| {
        u.exists() && u.is_completed() && !u.is_morphing()
            && u.unit_type() == UnitType::ZERG_HATCHERY
    }) {
        if hatchery.morph(UnitType::ZERG_LAIR) {
            bot.log(cx, &format!("lair: hatchery #{} morphing", hatchery.id()));
        } else {
            bot.build.mark_fail(UnitType::ZERG_LAIR, game.frame_count());
            game.send_text(&format!("lair morph failed: {}", game.last_error().name()));
            bot.log(cx, &format!("lair morph failed: {}", game.last_error().name()));
        }
    }
}

/// Tier-3 chain for the muta order: a drone-built queen's nest once the lair
/// is up, then the lair morphs on to a hive when the nest completes. Hive
/// itself enables nothing yet — ultra cavern is a later step — but the morph
/// takes long, so it starts early.
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
            issue_build(cx, bot, plan, "queen's nest");
        }
        return;
    }
    if nests > 0 && hives == 0 {
        let nest_done =
            counts.completed.get(&UnitType::ZERG_QUEENS_NEST).copied().unwrap_or(0) > 0;
        if nest_done && bot.build.can_retry(UnitType::ZERG_HIVE, game.frame_count()) {
            let me = game.self_player().expect("the bot plays a match, not a replay");
            if me.minerals() < UnitType::ZERG_HIVE.mineral_price()
                || me.gas() < UnitType::ZERG_HIVE.gas_price()
            {
                return;
            }
            // whatBuilds of the hive is the lair; the nest is only a
            // prerequisite, morphing it fails with Incompatible_UnitType.
            if let Some(lair) = bot.own_units.iter(cx).find(|u| {
                u.exists() && u.is_completed() && !u.is_morphing()
                    && u.unit_type() == UnitType::ZERG_LAIR
            }) {
                if lair.morph(UnitType::ZERG_HIVE) {
                    bot.log(cx, &format!("hive: lair #{} morphing", lair.id()));
                } else {
                    bot.build.mark_fail(UnitType::ZERG_HIVE, game.frame_count());
                    game.send_text(&format!("hive morph failed: {}", game.last_error().name()));
                    bot.log(cx, &format!("hive morph failed: {}", game.last_error().name()));
                }
            }
        }
    }
}

/// Builds the spire for the muta order once the lair is complete.
fn manage_spire<'game>(cx: Context<'_, 'game>, bot: &mut ZzzkBot<'game>) {
    if bot.strategy.order != Some(BuildOrder::MutaRush) {
        return;
    }
    let counts = crate::util::own_counts(&bot.own_units, cx);
    let lair_done = counts.completed.get(&UnitType::ZERG_LAIR).copied().unwrap_or(0) > 0;
    let spires = counts.all.get(&UnitType::ZERG_SPIRE).copied().unwrap_or(0);
    if !lair_done || spires > 0 {
        return;
    }
    let spire = UnitType::ZERG_SPIRE;
    if let Some(plan) = bot.build.plan(cx, &bot.own_units, &bot.scouting, spire) {
        issue_build(cx, bot, plan, "spire");
    }
}

/// Builds the hydralisk den for the hydra rush once the lair is complete
/// (the den itself only needs the pool, but the rush times with tier 2).
fn manage_den<'game>(cx: Context<'_, 'game>, bot: &mut ZzzkBot<'game>) {
    if bot.strategy.order != Some(BuildOrder::HydraRush) {
        return;
    }
    let counts = crate::util::own_counts(&bot.own_units, cx);
    let lair_done = counts.completed.get(&UnitType::ZERG_LAIR).copied().unwrap_or(0) > 0;
    let dens = counts
        .all
        .get(&UnitType::ZERG_HYDRALISK_DEN)
        .copied()
        .unwrap_or(0);
    if !lair_done || dens > 0 {
        return;
    }
    let den = UnitType::ZERG_HYDRALISK_DEN;
    if let Some(plan) = bot.build.plan(cx, &bot.own_units, &bot.scouting, den) {
        issue_build(cx, bot, plan, "den");
    }
}

/// Builds the ultralisk cavern for the late game once the hive is complete
/// (muta order: its tier-3 chain already reaches the hive).
fn manage_cavern<'game>(cx: Context<'_, 'game>, bot: &mut ZzzkBot<'game>) {
    if bot.strategy.order != Some(BuildOrder::MutaRush) {
        return;
    }
    let counts = crate::util::own_counts(&bot.own_units, cx);
    let hive_done = counts.completed.get(&UnitType::ZERG_HIVE).copied().unwrap_or(0) > 0;
    let caverns = counts
        .all
        .get(&UnitType::ZERG_ULTRALISK_CAVERN)
        .copied()
        .unwrap_or(0);
    if !hive_done || caverns > 0 {
        return;
    }
    let cavern = UnitType::ZERG_ULTRALISK_CAVERN;
    if let Some(plan) = bot.build.plan(cx, &bot.own_units, &bot.scouting, cavern) {
        issue_build(cx, bot, plan, "cavern");
    }
}

/// Base defense: creep colonies up to the strategy's sunken count, finished
/// ones morphed into sunken colonies. Returns that count, colonies included.
fn manage_sunkens<'game>(cx: Context<'_, 'game>, bot: &mut ZzzkBot<'game>) -> i32 {
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
                bot.log(cx, &format!("sunken: colony #{} morphing", colony.id()));
            }
        }
    }

    if have < want {
        let colony = UnitType::ZERG_CREEP_COLONY;
        if let Some(plan) = bot.build.plan(cx, &bot.own_units, &bot.scouting, colony) {
            issue_build(cx, bot, plan, "creep colony");
        }
    }
    have
}

/// Builds the extractor after the pool (not on a 4-pool all-in) and staffs
/// it with three drones once completed.
fn manage_gas<'game>(cx: Context<'_, 'game>, bot: &mut ZzzkBot<'game>) {
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
            issue_build(cx, bot, plan, "extractor");
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
                // The plan layer drives its reserved builders; a mining order
                // here would override the fresh build command.
                if self.build.is_builder(unit) {
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
