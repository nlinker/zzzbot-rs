//! Building placement and orders: the generic "build X" engine.

use std::collections::HashMap;

use bwapi::{Context, Position, TilePosition, Unit, UnitType};

use crate::{ZzzkBot, strategy::Opener};

/// Frames between retries after a failed placement.
const RETRY_FRAMES: i32 = 24;

/// Supply freed by a morphing extractor: its drone stops costing supply.
/// Cancelling the morph refunds the minerals and returns the drone.
pub fn extractor_trick<'game>(cx: Context<'_, 'game>, bot: &mut ZzzkBot<'game>, free_supply: i32) {
    let game = cx.game();
    // A ling egg is two zerglings; below that the bot is supply-blocked.
    let egg_supply = UnitType::ZERG_ZERGLING.supply_required() * 2;

    // Phase 2: the extractor morphs. Cancel only once the freed supply was
    // actually spent — a zergling egg is under way — or there is no larva
    // left to spend it on; cancelling on the freed supply alone would return
    // the drone before any egg starts and the trick loops on nothing.
    if let Some(extractor) = bot.trick_extractor {
        if extractor.exists() && extractor.is_morphing() {
            let egg_underway = bot
                .own_units
                .iter(cx)
                .any(|u| u.exists() && u.unit_type() == UnitType::ZERG_EGG);
            let larva_left = bot
                .own_units
                .iter(cx)
                .any(|u| u.exists() && u.is_idle() && u.unit_type() == UnitType::ZERG_LARVA);
            if !egg_underway && larva_left {
                return; // keep the drone consumed until the egg starts
            }
            let _ = extractor.cancel_morph();
            game.send_text("extractor trick: cancelled");
            bot.log(cx, "extractor trick: cancelled");
            bot.trick_extractor = None;
            return;
        }
        bot.trick_extractor = None;
        return;
    }

    // Phase 1: the builder was sent; wait for the morph to start. A walking
    // builder carries no build type for the whole trip — only an idle one
    // means the order died, anything else is still on its way.
    if let Some(builder) = bot.trick_builder {
        if builder.exists() && builder.unit_type() == UnitType::ZERG_EXTRACTOR {
            bot.trick_builder = None;
            bot.trick_extractor = Some(builder);
            return;
        }
        if builder.exists() && (builder.build_type() == UnitType::ZERG_EXTRACTOR || !builder.is_idle()) {
            return;
        }
        // The order was accepted in an earlier frame: BWAPI has no error for it now.
        let why = if builder.exists() {
            "builder idle without morphing"
        } else {
            "builder lost"
        };
        bot.trick_builder = None;
        bot.log(cx, &format!("extractor trick: order died: {why}"));
        return;
    }

    // Orphan fallback: a lost phase-1 track ("order died" while the queued
    // build command still lived) leaves a morphing extractor nobody cancels
    // — the drone is consumed and the supply never comes back. On the all-in
    // any morphing extractor IS the trick's: same cancel condition as
    // phase 2.
    if matches!(bot.strategy.opener, Opener::Pool4) {
        for unit in bot.own_units.iter(cx) {
            if unit.exists() && unit.is_morphing() && unit.unit_type() == UnitType::ZERG_EXTRACTOR {
                let egg_underway = bot
                    .own_units
                    .iter(cx)
                    .any(|u| u.exists() && u.unit_type() == UnitType::ZERG_EGG);
                let larva_left = bot
                    .own_units
                    .iter(cx)
                    .any(|u| u.exists() && u.is_idle() && u.unit_type() == UnitType::ZERG_LARVA);
                if egg_underway || !larva_left {
                    let _ = unit.cancel_morph();
                    bot.log(cx, "extractor trick: orphan cancelled");
                }
                return;
            }
        }
    }

    // Nothing under way. Start conditions: pool complete (lings morphable),
    // supply blocked, a free larva to spend the freed supply on, minerals
    // for the extractor AND the lings, and a buildable geyser — anything
    // less consumes the drone for nothing.
    if free_supply >= egg_supply {
        return;
    }
    // The gas extractor frees its drone's supply the same way; the trick only
    // makes sense before any extractor exists. The extractor order (gas) is
    // issued only outside the all-in, so outside the all-in the trick would
    // race it: the walking builder carries no build type yet, the check above
    // cannot see it, and both drones end up bound for the same geyser.
    if !matches!(bot.strategy.opener, Opener::Pool4) {
        return;
    }
    let pool_done = bot
        .own_units
        .iter(cx)
        .any(|u| u.exists() && u.is_completed() && u.unit_type() == UnitType::ZERG_SPAWNING_POOL);
    if !pool_done {
        return;
    }
    let me = game.self_player().expect("the bot plays a match, not a replay");
    let cost = UnitType::ZERG_EXTRACTOR.mineral_price() + UnitType::ZERG_ZERGLING.mineral_price() * 2;
    if me.minerals() < cost {
        return;
    }
    let larva_free = bot
        .own_units
        .iter(cx)
        .any(|u| u.exists() && u.is_idle() && u.unit_type() == UnitType::ZERG_LARVA);
    if !larva_free {
        return;
    }
    let base = bot.scouting.my_start;
    let home = Position::new(base.x * 32, base.y * 32);
    let Some(geyser) = game
        .geysers()
        .iter(cx)
        .filter(|&g| g.exists() && g.distance_to_position(home) < 25 * 32)
        .filter(|&g| game.can_build_here_with(g.tile_position(), UnitType::ZERG_EXTRACTOR, None, false))
        .min_by_key(|&g| g.distance_to_position(home))
    else {
        return;
    };
    let Some(builder) = bot.own_units.iter(cx).find(|u| {
        u.exists()
            && u.is_completed()
            && !u.is_morphing()
            && !u.is_constructing()
            && u.unit_type() == UnitType::ZERG_DRONE
            && !bot.scouting.is_scout(*u)
            && !bot.build.is_builder(*u)
            && !bot.gas_drones.contains(u)
    }) else {
        return;
    };
    match builder.build(UnitType::ZERG_EXTRACTOR, geyser.tile_position()) {
        Ok(()) => {
            game.send_text("extractor trick: started");
            bot.log(cx, &format!("extractor trick: started, builder #{}", builder.id()));
            bot.trick_builder = Some(builder);
        }
        Err(error) => bot.log(cx, &format!("extractor trick: order failed: {error}")),
    }
}

/// A build with its builder and tile chosen, ready to be ordered.
pub struct Plan<'game> {
    pub unit_type: UnitType,
    pub builder: Unit<'game>,
    pub tile: TilePosition,
}

/// Plans buildings: builder choice, tile choice, retry throttling.
#[derive(Default)]
pub struct BuildEngine<'game> {
    /// Frame of the last failed attempt, per building type.
    last_fail: HashMap<UnitType, i32>,
    /// Drone sent to build each type: ordered, but not morphing yet. A
    /// travelling builder reads neither `is_constructing` nor an egg, so
    /// without this each frame would send a fresh drone to the same tile.
    builders: HashMap<UnitType, Unit<'game>>,
    /// Reserved tiles, parallel to `builders`: two walking builders in the
    /// same frame both pass `can_build_here` on the same free tile, and one
    /// of them arrives to find it taken and wedges.
    tiles: HashMap<UnitType, TilePosition>,
}

impl<'game> BuildEngine<'game> {
    pub fn new() -> Self {
        BuildEngine::default()
    }

    /// Drops the reservation of a type: the walking builder or its tile no
    /// longer holds the queue slot.
    fn release(&mut self, unit_type: UnitType) {
        self.builders.remove(&unit_type);
        self.tiles.remove(&unit_type);
    }

    /// Marks an attempt failed (the build order was rejected) to throttle retries.
    pub fn mark_fail(&mut self, unit_type: UnitType, frame: i32) {
        self.last_fail.insert(unit_type, frame);
        self.release(unit_type);
    }

    /// True when enough frames passed since the last failed attempt.
    pub fn can_retry(&self, unit_type: UnitType, frame: i32) -> bool {
        match self.last_fail.get(&unit_type) {
            Some(&failed_at) => frame >= failed_at + RETRY_FRAMES,
            None => true,
        }
    }

    pub fn is_builder(&self, unit: Unit<'game>) -> bool {
        self.builders.values().any(|&builder| builder == unit)
    }

    /// Overlay state of a pending build: throttle phase and reserved builder.
    pub fn debug_line(&self, unit_type: UnitType, frame: i32) -> String {
        let throttle = match self.last_fail.get(&unit_type) {
            Some(&failed_at) => {
                let left = failed_at + RETRY_FRAMES - frame;
                if left > 0 {
                    format!("wait {left}")
                } else {
                    String::from("ready")
                }
            }
            None => String::from("ready"),
        };
        let builder = self
            .builders
            .get(&unit_type)
            .filter(|b| b.exists())
            .map(|b| format!(" builder #{}", b.id()))
            .unwrap_or_default();
        format!("[{throttle}{builder}]")
    }
}

/// Picks a builder and tile when retry allows. The caller builds. The filters
/// exclude every drone with another job: a geyser stripped of its workers
/// stalls the gas income for the rest of the match.
pub fn plan<'game>(cx: Context<'_, 'game>, bot: &mut ZzzkBot<'game>, unit_type: UnitType) -> Option<Plan<'game>> {
    let game = cx.game();
    let me = game.self_player().expect("the bot plays a match, not a replay");
    if me.minerals() < unit_type.mineral_price() || me.gas() < unit_type.gas_price() {
        return None;
    }
    if !bot.build.can_retry(unit_type, game.frame_count()) {
        return None;
    }

    // Already on it: the reserved builder walks there or morphs. If it idles
    // instead, the order died (blocked path, lost target): drop the
    // reservation and let the throttle schedule a retry, else it wedges.
    if let Some(&builder) = bot.build.builders.get(&unit_type) {
        if builder.exists() && builder.unit_type() == UnitType::ZERG_DRONE {
            if !builder.is_idle() {
                return None;
            }
            bot.build.release(unit_type);
            bot.build.last_fail.insert(unit_type, game.frame_count());
            return None;
        }
        bot.build.release(unit_type);
    }

    let base = bot.scouting.my_start;
    let builder = pick_builder(cx, bot, Position::new(base.x * 32, base.y * 32))?;

    // A refinery goes on a geyser, not on a free tile. `geysers()` order is
    // arbitrary, so among the candidates pick the one nearest home —
    // otherwise the extractor may land on a distant expansion geyser and gas
    // income crawls for the whole match.
    let tile = if unit_type.is_refinery() {
        let home = Position::new(base.x * 32, base.y * 32);
        game.geysers()
            .iter(cx)
            .filter(|&g| g.exists() && g.distance_to_position(home) < 25 * 32)
            .filter(|&g| game.can_build_here_with(g.tile_position(), unit_type, Some(builder), false))
            .filter(|&g| !bot.build.tiles.values().any(|&t| t == g.tile_position()))
            .min_by_key(|&g| g.distance_to_position(home))
            .map(|g| g.tile_position())
    } else {
        let tile = game.build_location_with(unit_type, base, 64, unit_type.requires_creep());
        (game.can_build_here_with(tile, unit_type, Some(builder), false))
            .then_some(tile)
            // Two walking builders in the same frame both pass
            // `can_build_here` on one free tile; the loser arrives to find
            // it taken and wedges with a dead order.
            .filter(|tile| !bot.build.tiles.values().any(|&t| t == *tile))
    };
    let Some(tile) = tile else {
        bot.build.last_fail.insert(unit_type, game.frame_count());
        return None;
    };
    bot.build.builders.insert(unit_type, builder);
    bot.build.tiles.insert(unit_type, tile);
    Some(Plan { unit_type, builder, tile })
}

/// Nearest free drone: not a scout, not morphing, not already building, not
/// a gas worker, not the trick builder.
fn pick_builder<'game>(cx: Context<'_, 'game>, bot: &ZzzkBot<'game>, near: Position) -> Option<Unit<'game>> {
    bot.own_units
        .iter(cx)
        .filter(|u| {
            u.exists()
                && u.is_completed()
                && !u.is_morphing()
                && !u.is_constructing()
                && u.unit_type() == UnitType::ZERG_DRONE
                && !bot.scouting.is_scout(*u)
                && !bot.build.is_builder(*u)
                && !bot.gas_drones.contains(u)
                && bot.trick_builder != Some(*u)
        })
        .min_by_key(|u| u.distance_to_position(near))
}
