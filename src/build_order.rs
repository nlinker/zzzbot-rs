//! Building placement and orders: the generic "build X" engine.

use std::collections::HashMap;

use crate::scouting::Scouting;
use crate::ZzzkBot;
use bwapi::{Context, Position, TilePosition, Unit, UnitSet, UnitType};

/// Frames between retries after a failed placement.
const RETRY_FRAMES: i32 = 24;

/// Supply freed by a morphing extractor: its drone stops costing supply.
/// Cancelling the morph refunds the minerals and returns the drone.
pub fn extractor_trick<'game>(
    cx: Context<'_, 'game>,
    bot: &mut ZzzkBot<'game>,
    free_supply: i32,
) {
    let game = cx.game();
    // A ling egg is two zerglings; below that the bot is supply-blocked.
    let egg_supply = UnitType::ZERG_ZERGLING.supply_required() * 2;

    // Phase 2: the extractor morphs; cancel it once the larva can fit.
    if let Some(extractor) = bot.trick_extractor {
        if extractor.exists() && extractor.is_morphing() {
            if free_supply < egg_supply {
                return; // keep the drone consumed until the trick is needed
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

    // Phase 1: the builder was sent; wait for the morph to start.
    if let Some(builder) = bot.trick_builder {
        if builder.exists() && builder.unit_type() == UnitType::ZERG_EXTRACTOR {
            bot.trick_builder = None;
            bot.trick_extractor = Some(builder);
            return;
        }
        if builder.exists() && builder.build_type() == UnitType::ZERG_EXTRACTOR {
            return;
        }
        bot.trick_builder = None; // dead or the order failed
        return;
    }

    // Nothing under way: start the trick while blocked, if a geyser is free.
    if free_supply >= egg_supply {
        return;
    }
    let base = bot.scouting.my_start;
    let home = Position::new(base.x * 32, base.y * 32);
    let Some(geyser) = game
        .geysers()
        .iter(cx)
        .find(|&g| g.exists() && g.distance_to_position(home) < 25 * 32)
    else {
        return;
    };
    let Some(builder) = bot
        .own_units
        .iter(cx)
        .find(|u| {
            u.exists() && u.is_completed() && !u.is_morphing() && !u.is_constructing()
                && u.unit_type() == UnitType::ZERG_DRONE
                && !bot.scouting.is_scout(*u)
                && !bot.build.is_builder(*u)
                && !bot.gas_drones.contains(u)
        })
    else {
        return;
    };
    if builder.build(UnitType::ZERG_EXTRACTOR, geyser.tile_position()) {
        game.send_text("extractor trick: started");
        bot.log(cx, &format!("extractor trick: started, builder #{}", builder.id()));
        bot.trick_builder = Some(builder);
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
}

impl<'game> BuildEngine<'game> {
    pub fn new() -> Self {
        BuildEngine::default()
    }

    /// Marks an attempt failed (the build order was rejected) to throttle retries.
    pub fn mark_fail(&mut self, unit_type: UnitType, frame: i32) {
        self.last_fail.insert(unit_type, frame);
        self.builders.remove(&unit_type);
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

    /// Picks a builder and tile when retry allows. The caller builds.
    pub fn plan(
        &mut self,
        cx: Context<'_, 'game>,
        own: &UnitSet<'game>,
        scouting: &Scouting<'game>,
        unit_type: UnitType,
    ) -> Option<Plan<'game>> {
        let game = cx.game();
        let me = game.self_player().expect("the bot plays a match, not a replay");
        if me.minerals() < unit_type.mineral_price() {
            return None;
        }
        if !self.can_retry(unit_type, game.frame_count()) {
            return None;
        }

        // Already on it: the reserved builder walks there or morphs. If it
        // idles instead, the order died (blocked path, lost target): drop the
        // reservation and let the throttle schedule a retry, else it wedges.
        if let Some(&builder) = self.builders.get(&unit_type) {
            if builder.exists() && builder.unit_type() == UnitType::ZERG_DRONE {
                if !builder.is_idle() {
                    return None;
                }
                self.builders.remove(&unit_type);
                self.last_fail.insert(unit_type, game.frame_count());
                return None;
            }
            self.builders.remove(&unit_type);
        }

        let base = scouting.my_start;
        let builder = pick_builder(cx, own, scouting, Position::new(base.x * 32, base.y * 32))?;

        // A refinery goes on a geyser, not on a free tile.
        let tile = if unit_type.is_refinery() {
            let home = Position::new(base.x * 32, base.y * 32);
            game.geysers()
                .iter(cx)
                .filter(|&g| g.exists() && g.distance_to_position(home) < 25 * 32)
                .map(|g| g.tile_position())
                .find(|&tile| game.can_build_here_with(tile, unit_type, Some(builder), false))
        } else {
            let tile = game.build_location_with(unit_type, base, 64, unit_type.requires_creep());
            (game.can_build_here_with(tile, unit_type, Some(builder), false)).then_some(tile)
        };
        let Some(tile) = tile else {
            self.last_fail.insert(unit_type, game.frame_count());
            return None;
        };
        self.builders.insert(unit_type, builder);
        Some(Plan { unit_type, builder, tile })
    }
}

/// Nearest free drone: not a scout, not morphing, not already building.
fn pick_builder<'game>(
    cx: Context<'_, 'game>,
    own: &UnitSet<'game>,
    scouting: &Scouting<'game>,
    near: Position,
) -> Option<Unit<'game>> {
    own.iter(cx)
        .filter(|u| {
            u.exists() && u.is_completed() && !u.is_morphing() && !u.is_constructing()
                && u.unit_type() == UnitType::ZERG_DRONE
                && !scouting.is_scout(*u)
        })
        .min_by_key(|u| u.distance_to_position(near))
}
