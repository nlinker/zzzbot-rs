//! Worker and depot orders: demo mining until the full gathering discipline.

use bwapi::{Context, CoordinateType, Position, Unit, UnitType, UpgradeType};

use crate::{ZzzBot, army, build_order, strategy::BuildOrder};

/// Maximum distance from an owned resource depot to a mineral patch (pixels).
const MINERAL_DEPOT_RADIUS: i32 = 10 * 32;

/// Worker-rush defense: how far from the start a ground unit counts as a
/// raid on the mineral line (pixels).
const RAID_RADIUS: i32 = 10 * 32;

/// Drones on one geyser.
const GAS_WORKERS: usize = 3;

/// Worker ceiling: mining saturates well below this, the rest of the
/// minerals and larvae belongs to the army.
const DRONE_CAP: i32 = 40;

/// Issues a planned build and logs the decision either way: builder and tile
/// on success, the BWAPI error on failure.
fn issue_build<'game>(cx: Context<'_, 'game>, bot: &mut ZzzBot<'game>, plan: build_order::Plan<'game>, label: &str) {
    let game = cx.game();
    let at = format!("{} at {},{}", label, plan.tile.x, plan.tile.y);
    match plan.builder.build(plan.unit_type, plan.tile) {
        Ok(()) => bot.log(cx, &format!("{at}: builder #{}", plan.builder.id())),
        Err(error) => {
            bot.build.mark_fail(plan.unit_type, game.frame_count());
            bot.log(cx, &format!("{at}: failed: {error}"));
        }
    }
}

/// The macro layer: issues build orders by the current strategy.
pub fn macro_orders<'game>(cx: Context<'_, 'game>, bot: &mut ZzzBot<'game>) {
    let game = cx.game();
    let counts = crate::util::own_counts(&bot.own_units, cx);
    let have_pool = counts.all.get(&UnitType::ZERG_SPAWNING_POOL).copied().unwrap_or(0) > 0;
    if !have_pool {
        // Top of the queue: the pool command applies before anything else
        // spends the same minerals. The rest of the macro below keeps
        // running while the pool is absent or dead — an early return here
        // once froze a whole base (no gas refill, no sunkens) for a match.
        let pool = UnitType::ZERG_SPAWNING_POOL;
        if let Some(plan) = build_order::plan(cx, bot, pool) {
            issue_build(cx, bot, plan, "pool");
        }
    }

    // While the pool is still to start, its 200 minerals are the budget: an
    // overlord morph here would eat them mid-flight of the builder.
    let me = game.self_player().expect("the bot plays a match, not a replay");
    let free = me.supply_total() - counts.supply_used;
    build_order::extractor_trick(cx, bot, free);
    army::supply_overlords(cx, bot, &counts);

    // Tech and economy before army morphs: BWAPI applies queued commands at
    // the end of the frame in issue order, so a muta queued earlier could
    // drink the gas right before the cavern command applies.
    manage_gas(cx, bot);
    manage_hatcheries(cx, bot);
    manage_lair(cx, bot);
    manage_tier3(cx, bot);
    manage_spire(cx, bot);
    manage_den(cx, bot);
    manage_cavern(cx, bot);
    manage_upgrades(cx, bot);
    let sunkens = manage_sunkens(cx, bot);
    let made = army::morph_lings(cx, bot);
    army::morph_mutas(cx, bot);
    army::morph_hydras(cx, bot);
    army::morph_ultras(cx, bot);
    let extra = if made > 0 {
        format!(", lings +{made}")
    } else {
        String::new()
    };
    let hatch_count = counts.all.get(&UnitType::ZERG_HATCHERY).copied().unwrap_or(0)
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
        "dbg nest {} colony {} hatch {} spire {} | m {} g {}",
        bot.build.debug_line(UnitType::ZERG_QUEENS_NEST, game.frame_count()),
        bot.build.debug_line(UnitType::ZERG_CREEP_COLONY, game.frame_count()),
        bot.build.debug_line(UnitType::ZERG_HATCHERY, game.frame_count()),
        bot.build.debug_line(UnitType::ZERG_SPIRE, game.frame_count()),
        me.minerals(),
        me.gas(),
    );
    game.draw_text(CoordinateType::Screen, 10, 92, &dbg);
}

/// Second hatchery for non-all-in orders: one is not enough larva flow.
fn manage_hatcheries<'game>(cx: Context<'_, 'game>, bot: &mut ZzzBot<'game>) {
    if bot.strategy.order == Some(BuildOrder::Pool4) {
        return; // all-in: minerals go into zerglings, not bases
    }
    let counts = crate::util::own_counts(&bot.own_units, cx);
    // After the pool: its 200 minerals come first, a hatchery at 150 would
    // otherwise steal the budget while the pool builder waits.
    let have_pool = counts.all.get(&UnitType::ZERG_SPAWNING_POOL).copied().unwrap_or(0) > 0;
    let bases = counts.all.get(&UnitType::ZERG_HATCHERY).copied().unwrap_or(0)
        + counts.all.get(&UnitType::ZERG_LAIR).copied().unwrap_or(0)
        + counts.all.get(&UnitType::ZERG_HIVE).copied().unwrap_or(0);
    if bases >= 2 || !have_pool {
        return;
    }
    let hatchery = UnitType::ZERG_HATCHERY;
    if let Some(plan) = build_order::plan(cx, bot, hatchery) {
        issue_build(cx, bot, plan, "hatchery");
    }
}

/// Morphs the first hatchery into a lair once the pool and gas are up and
/// the order wants tier-2 tech (mutas, hydras and the speedling upgrade
/// path all need it).
fn manage_lair<'game>(cx: Context<'_, 'game>, bot: &mut ZzzBot<'game>) {
    let wants_t2 = matches!(
        bot.strategy.order,
        Some(BuildOrder::Speedling) | Some(BuildOrder::MutaRush) | Some(BuildOrder::HydraRush)
    );
    if !wants_t2 {
        return;
    }
    let counts = crate::util::own_counts(&bot.own_units, cx);
    let pool_done = counts
        .completed
        .get(&UnitType::ZERG_SPAWNING_POOL)
        .copied()
        .unwrap_or(0)
        > 0;
    let gas = counts.completed.get(&UnitType::ZERG_EXTRACTOR).copied().unwrap_or(0) > 0;
    let tier2 = counts.all.get(&UnitType::ZERG_LAIR).copied().unwrap_or(0)
        + counts.all.get(&UnitType::ZERG_HIVE).copied().unwrap_or(0)
        + counts.all.get(&UnitType::ZERG_QUEENS_NEST).copied().unwrap_or(0);
    if !pool_done || !gas || tier2 > 0 {
        return;
    }
    let game = cx.game();
    if !bot.build.can_retry(UnitType::ZERG_LAIR, game.frame_count()) {
        return;
    }
    let me = game.self_player().expect("the bot plays a match, not a replay");
    if me.minerals() < UnitType::ZERG_LAIR.mineral_price() || me.gas() < UnitType::ZERG_LAIR.gas_price() {
        return;
    }
    if let Some(hatchery) = bot
        .own_units
        .iter(cx)
        .find(|u| u.exists() && u.is_completed() && !u.is_morphing() && u.unit_type() == UnitType::ZERG_HATCHERY)
    {
        match hatchery.morph(UnitType::ZERG_LAIR) {
            Ok(()) => bot.log(cx, &format!("lair: hatchery #{} morphing", hatchery.id())),
            Err(error) => {
                bot.build.mark_fail(UnitType::ZERG_LAIR, game.frame_count());
                game.send_text(&format!("lair morph failed: {error}"));
                bot.log(cx, &format!("lair morph failed: {error}"));
            }
        }
    }
}

/// Tier-3 chain: a drone-built queen's nest once the lair is up (the hydra
/// order waits for its den), then the lair morphs on to a hive when the nest
/// completes. The hive itself gates nothing here — upgrades and the ultra
/// cavern hang off their own gates — but the morph takes long, so it starts
/// early.
fn manage_tier3<'game>(cx: Context<'_, 'game>, bot: &mut ZzzBot<'game>) {
    // Speedling joins for the late game: once the attack stalls on a bank of
    // minerals, the hive unlocks Adrenal Glands (cracklings for free).
    if !matches!(
        bot.strategy.order,
        Some(BuildOrder::Speedling) | Some(BuildOrder::MutaRush) | Some(BuildOrder::HydraRush)
    ) {
        return;
    }
    let game = cx.game();
    let counts = crate::util::own_counts(&bot.own_units, cx);
    let lair_done = counts.completed.get(&UnitType::ZERG_LAIR).copied().unwrap_or(0) > 0;
    let dens = counts.all.get(&UnitType::ZERG_HYDRALISK_DEN).copied().unwrap_or(0);
    let nests = counts.all.get(&UnitType::ZERG_QUEENS_NEST).copied().unwrap_or(0);
    let hives = counts.all.get(&UnitType::ZERG_HIVE).copied().unwrap_or(0);

    // The nest is a drone building (whatBuilds = Drone, Lair is a
    // prerequisite only); morphing a lair would silently fail.
    let den_wait = bot.strategy.order == Some(BuildOrder::HydraRush) && dens == 0;
    if lair_done && !den_wait && nests == 0 {
        let nest = UnitType::ZERG_QUEENS_NEST;
        if let Some(plan) = build_order::plan(cx, bot, nest) {
            issue_build(cx, bot, plan, "queen's nest");
        }
        return;
    }
    if nests > 0 && hives == 0 {
        let nest_done = counts.completed.get(&UnitType::ZERG_QUEENS_NEST).copied().unwrap_or(0) > 0;
        if nest_done && bot.build.can_retry(UnitType::ZERG_HIVE, game.frame_count()) {
            let me = game.self_player().expect("the bot plays a match, not a replay");
            if me.minerals() < UnitType::ZERG_HIVE.mineral_price() || me.gas() < UnitType::ZERG_HIVE.gas_price() {
                if game.frame_count() % 300 == 0 {
                    bot.log(cx, &format!("hive blocked: m {}, g {}", me.minerals(), me.gas()));
                }
                return;
            }
            // whatBuilds of the hive is the lair; the nest is only a
            // prerequisite, morphing it fails with Incompatible_UnitType.
            if let Some(lair) = bot
                .own_units
                .iter(cx)
                .find(|u| u.exists() && u.is_completed() && !u.is_morphing() && u.unit_type() == UnitType::ZERG_LAIR)
            {
                match lair.morph(UnitType::ZERG_HIVE) {
                    Ok(()) => bot.log(cx, &format!("hive: lair #{} morphing", lair.id())),
                    Err(error) => {
                        bot.build.mark_fail(UnitType::ZERG_HIVE, game.frame_count());
                        game.send_text(&format!("hive morph failed: {error}"));
                        bot.log(cx, &format!("hive morph failed: {error}"));
                    }
                }
            }
        }
    }
}

/// Builds the spire for the muta order once the lair is complete.
fn manage_spire<'game>(cx: Context<'_, 'game>, bot: &mut ZzzBot<'game>) {
    let game = cx.game();
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
    if let Some(plan) = build_order::plan(cx, bot, spire) {
        issue_build(cx, bot, plan, "spire");
    } else if game.frame_count() % 300 == 0 {
        // Every rejection path in `plan` is silent; this is the only window
        // into "why is there no spire".
        let me = game.self_player().expect("the bot plays a match, not a replay");
        bot.log(
            cx,
            &format!(
                "spire blocked: m {}, g {}, lair_done {lair_done}",
                me.minerals(),
                me.gas()
            ),
        );
    }
}

/// Builds the hydralisk den for the hydra rush once the lair is complete
/// (the den itself only needs the pool, but the rush times with tier 2).
fn manage_den<'game>(cx: Context<'_, 'game>, bot: &mut ZzzBot<'game>) {
    if bot.strategy.order != Some(BuildOrder::HydraRush) {
        return;
    }
    let counts = crate::util::own_counts(&bot.own_units, cx);
    let lair_done = counts.completed.get(&UnitType::ZERG_LAIR).copied().unwrap_or(0) > 0;
    let dens = counts.all.get(&UnitType::ZERG_HYDRALISK_DEN).copied().unwrap_or(0);
    if !lair_done || dens > 0 {
        return;
    }
    let den = UnitType::ZERG_HYDRALISK_DEN;
    if let Some(plan) = build_order::plan(cx, bot, den) {
        issue_build(cx, bot, plan, "den");
    }
}

/// Builds the ultralisk cavern for the late game once the hive is complete
/// (muta order: its tier-3 chain already reaches the hive).
fn manage_cavern<'game>(cx: Context<'_, 'game>, bot: &mut ZzzBot<'game>) {
    if bot.strategy.order != Some(BuildOrder::MutaRush) {
        return;
    }
    let counts = crate::util::own_counts(&bot.own_units, cx);
    let hive_done = counts.completed.get(&UnitType::ZERG_HIVE).copied().unwrap_or(0) > 0;
    let caverns = counts.all.get(&UnitType::ZERG_ULTRALISK_CAVERN).copied().unwrap_or(0);
    if !hive_done || caverns > 0 {
        return;
    }
    let cavern = UnitType::ZERG_ULTRALISK_CAVERN;
    if let Some(plan) = build_order::plan(cx, bot, cavern) {
        issue_build(cx, bot, plan, "cavern");
    }
}

/// Ling upgrades at the spawning pool: Metabolic Boost once the lair is up
/// (the Speedling path, or any late game that reached tier 2), Adrenal
/// Glands at the hive.
fn manage_upgrades<'game>(cx: Context<'_, 'game>, bot: &mut ZzzBot<'game>) {
    let game = cx.game();
    let me = game.self_player().expect("the bot plays a match, not a replay");
    let counts = crate::util::own_counts(&bot.own_units, cx);
    // The lair morphs away into the hive, so tier 2 is either of the two.
    let tier2_done = counts.completed.get(&UnitType::ZERG_LAIR).copied().unwrap_or(0) > 0
        || counts.completed.get(&UnitType::ZERG_HIVE).copied().unwrap_or(0) > 0;
    let hive_done = counts.completed.get(&UnitType::ZERG_HIVE).copied().unwrap_or(0) > 0;

    // Metabolic first: Adrenal only makes sense on top of it.
    let want = if tier2_done
        && (bot.strategy.order == Some(BuildOrder::Speedling) || hive_done)
        && me.upgrade_level(UpgradeType::METABOLIC_BOOST) == 0
    {
        UpgradeType::METABOLIC_BOOST
    } else if hive_done && me.upgrade_level(UpgradeType::ADRENAL_GLANDS) == 0 {
        UpgradeType::ADRENAL_GLANDS
    } else {
        return;
    };
    if me.is_upgrading(want) {
        return;
    }
    if me.minerals() < want.mineral_price() || me.gas() < want.gas_price() {
        if game.frame_count() % 600 == 0 {
            bot.log(
                cx,
                &format!("upgrade {} blocked: m {}, g {}", want.name(), me.minerals(), me.gas()),
            );
        }
        return;
    }
    if let Some(pool) = bot
        .own_units
        .iter(cx)
        .find(|u| u.exists() && u.is_completed() && u.is_idle() && u.unit_type() == UnitType::ZERG_SPAWNING_POOL)
    {
        match pool.upgrade(want) {
            Ok(()) => bot.log(cx, &format!("upgrade: {} researching", want.name())),
            Err(error) => bot.log(cx, &format!("upgrade failed: {error}")),
        }
    }
}

/// Base defense: creep colonies up to the strategy's sunken count, finished
/// ones morphed into sunken colonies. Returns that count, colonies included.
fn manage_sunkens<'game>(cx: Context<'_, 'game>, bot: &mut ZzzBot<'game>) -> i32 {
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
    let pool_done = counts
        .completed
        .get(&UnitType::ZERG_SPAWNING_POOL)
        .copied()
        .unwrap_or(0)
        > 0;
    if pool_done {
        for colony in idle_colonies {
            if have >= want {
                break;
            }
            if colony.morph(UnitType::ZERG_SUNKEN_COLONY).is_ok() {
                have += 1;
                bot.log(cx, &format!("sunken: colony #{} morphing", colony.id()));
            }
        }
    }

    // Colonies require the pool (as does the later morph), and the pool's
    // 200 minerals come first.
    if have < want && pool_done {
        let colony = UnitType::ZERG_CREEP_COLONY;
        if let Some(plan) = build_order::plan(cx, bot, colony) {
            issue_build(cx, bot, plan, "creep colony");
        }
    }
    have
}

/// Builds the extractor after the pool (not on a 4-pool all-in) and staffs
/// it with three drones once completed.
fn manage_gas<'game>(cx: Context<'_, 'game>, bot: &mut ZzzBot<'game>) {
    // Dead drones and stale refinery leave the roster each frame; both events
    // are logged — a silent roster leak was hard to diagnose from the log.
    let before = bot.gas_drones.len();
    bot.gas_drones.retain(|d| d.exists());
    if bot.gas_drones.len() != before {
        bot.log(
            cx,
            &format!(
                "gas roster: lost {} drone(s), {} left",
                before - bot.gas_drones.len(),
                bot.gas_drones.len()
            ),
        );
    }
    let had_refinery = bot.refinery.is_some();
    bot.refinery = bot
        .own_units
        .iter(cx)
        .find(|u| u.exists() && u.is_completed() && u.unit_type().is_refinery());
    if had_refinery && bot.refinery.is_none() {
        bot.log(cx, "gas: refinery lost");
    }

    let all_in = bot.strategy.order == Some(BuildOrder::Pool4);
    let counts = crate::util::own_counts(&bot.own_units, cx);
    let refineries = counts.all.get(&UnitType::ZERG_EXTRACTOR).copied().unwrap_or(0);
    // After the pool, like the hatchery: the extractor must not eat the
    // pool's 200 minerals.
    let have_pool = counts.all.get(&UnitType::ZERG_SPAWNING_POOL).copied().unwrap_or(0) > 0;
    if !all_in && have_pool && refineries == 0 {
        let extractor = UnitType::ZERG_EXTRACTOR;
        if let Some(plan) = build_order::plan(cx, bot, extractor) {
            issue_build(cx, bot, plan, "extractor");
        }
    }

    // Staff the completed refinery: reassign the drones nearest to it. The
    // all-in skips this too: its only extractor is a supply trick, staffing
    // it would empty the mineral line (one match ended with every drone on
    // gas and zero income).
    let all_in_staff = bot.strategy.order == Some(BuildOrder::Pool4);
    if all_in_staff {
        return;
    }
    let Some(refinery) = bot.refinery else { return };
    while bot.gas_drones.len() < GAS_WORKERS {
        let Some(worker) = bot
            .own_units
            .iter(cx)
            .filter(|u| {
                u.exists()
                    && u.is_completed()
                    && !u.is_morphing()
                    && u.unit_type() == UnitType::ZERG_DRONE
                    && !bot.scouting.is_scout(*u)
                    && !bot.build.is_builder(*u)
                    && !bot.gas_drones.contains(u)
            })
            .min_by_key(|u| u.distance_to_unit(refinery))
        else {
            break;
        };
        if worker.gather(refinery).is_ok() {
            bot.log(cx, &format!("gas roster: +drone #{}", worker.id()));
            bot.gas_drones.push(worker);
        } else {
            break;
        }
    }
}

/// Worker-rush defense: enemy ground units inside the mineral line pull the
/// nearest miners into the fight, two per raider. Gas drones keep the geyser
/// running (the gather order in `give_orders` would override them anyway);
/// pulled drones go idle once the raiders die and return to mining.
pub fn defend_base<'game>(cx: Context<'_, 'game>, bot: &mut ZzzBot<'game>) {
    let game = cx.game();
    let Some(enemy) = bot.scouting.enemy else { return };
    let base = bot.scouting.my_start;
    let home = Position::new(base.x * 32, base.y * 32);
    let raiders: Vec<_> = game
        .units_in_radius(home, RAID_RADIUS)
        .filter(|u| u.exists() && u.player() == enemy && !u.is_flying() && u.unit_type() != UnitType::ZERG_OVERLORD)
        .collect();
    if raiders.is_empty() {
        return;
    }
    let announced = game.frame_count() % 120 == 0;
    if announced {
        bot.log(cx, &format!("defend: {} raiders in the mineral line", raiders.len()));
    }
    let need = (raiders.len() as i32 * 2).min(10);
    let mut fighting = 0;
    for unit in bot.own_units.iter(cx) {
        if fighting >= need {
            break;
        }
        if !unit.exists()
            || unit.unit_type() != UnitType::ZERG_DRONE
            || unit.is_morphing()
            || bot.build.is_builder(unit)
            || bot.scouting.is_scout(unit)
            || bot.trick_builder == Some(unit)
            || bot.gas_drones.contains(&unit)
        {
            continue;
        }
        let Some(target) = raiders.iter().copied().min_by_key(|&r| unit.distance_to_unit(r)) else {
            break;
        };
        if unit.attack_unit(target).is_ok() {
            fighting += 1;
            if announced {
                bot.log(cx, &format!("defend: drone #{} attacks #{}", unit.id(), target.id()));
            }
        }
    }
}

impl<'game> ZzzBot<'game> {
    /// Idle workers mine minerals near completed owned depots; depots train
    /// workers up to the cap — past it, larva and minerals go to the army.
    pub fn give_orders(&mut self, cx: Context<'_, 'game>) {
        let game = cx.game();
        let counts = crate::util::own_counts(&self.own_units, cx);
        let drones = counts.all.get(&UnitType::ZERG_DRONE).copied().unwrap_or(0);
        // Mining discipline: every order below is issued only to an idle
        // worker; a busy one keeps its order (gather cycles by itself, and
        // re-issuing would reset a live walk). The depot list is built once
        // per frame — the mineral filter runs per idle drone otherwise.
        let depots: Vec<Unit<'_>> = self
            .own_units
            .iter(cx)
            .filter(|u| u.exists() && u.is_completed() && u.unit_type().is_resource_depot())
            .collect();
        // Patches claimed this frame by an idle drone (see the mineral pick).
        let mut taken: Vec<Unit<'_>> = Vec::new();
        for unit in self.own_units.iter(cx) {
            let unit_type = unit.unit_type();
            if unit_type.is_worker() {
                if !unit.is_idle() {
                    continue;
                }
                // The plan layer drives its reserved builders; a mining order
                // here would override the fresh build command.
                if self.build.is_builder(unit) || self.trick_builder == Some(unit) {
                    continue;
                }
                // Gas drones cycle between the refinery and the depot.
                if self.gas_drones.contains(&unit) {
                    if unit.is_carrying_gas() {
                        let _ = unit.return_cargo();
                    } else if let Some(refinery) = self.refinery {
                        if let Err(error) = unit.gather(refinery) {
                            if game.frame_count() % 300 == 0 {
                                self.log(cx, &format!("gas: drone #{} gather failed: {error}", unit.id()));
                            }
                        }
                    } else if game.frame_count() % 300 == 0 {
                        self.log(cx, "gas: roster drone but no refinery");
                    }
                    continue;
                }
                if unit.is_carrying_gas() || unit.is_carrying_minerals() {
                    let _ = unit.return_cargo();
                    continue;
                }
                // Only use minerals near an existing, completed owned depot.
                // A scout returning from the map must not start mining at an
                // unoccupied expansion merely because those minerals are closer.
                // Patches already taken this frame are skipped: every drone
                // spreads to its own nearest free patch instead of all
                // piling onto the globally closest one.
                let near_depot = |mineral: Unit<'game>| {
                    mineral.exists()
                        && depots
                            .iter()
                            .any(|depot| depot.distance_to_unit(mineral) < MINERAL_DEPOT_RADIUS)
                };
                let closest = self
                    .minerals
                    .iter(cx)
                    .filter(|&mineral| near_depot(mineral) && !taken.contains(&mineral))
                    .min_by_key(|&mineral| unit.distance_to_unit(mineral))
                    // Every patch has a worker: share the nearest one.
                    .or_else(|| {
                        self.minerals
                            .iter(cx)
                            .filter(|&mineral| near_depot(mineral))
                            .min_by_key(|&mineral| unit.distance_to_unit(mineral))
                    });
                if let Some(mineral) = closest {
                    if !taken.contains(&mineral) {
                        taken.push(mineral);
                    }
                    let _ = unit.right_click_unit(mineral);
                }
            } else if unit_type.is_resource_depot() && !unit.is_training() {
                // The all-in needs every larva for lings — with one deliberate
                // exception: after the pool order is placed, a single extra
                // drone (50m). Two larvae remain, the hatchery regrows the
                // third, so pool completion meets three larvae = six lings.
                let all_in = self.strategy.order == Some(BuildOrder::Pool4);
                let pool4_drone = all_in
                    && !self.pool4_drone
                    && counts.all.get(&UnitType::ZERG_SPAWNING_POOL).copied().unwrap_or(0) > 0
                    && game
                        .self_player()
                        .map(|me| me.minerals() >= UnitType::ZERG_DRONE.mineral_price())
                        .unwrap_or(false);
                if drones < DRONE_CAP && (!all_in || pool4_drone) {
                    if all_in {
                        if unit.train(unit_type.race().worker()).is_ok() {
                            self.pool4_drone = true;
                        }
                    } else {
                        let _ = unit.train(unit_type.race().worker());
                    }
                }
            }
        }
    }
}
