//! Worker and depot orders: demo mining until the full gathering discipline.

use crate::ZzzkBot;
use bwapi::{Context, CoordinateType, UnitType};

/// Maximum distance from an owned resource depot to a mineral patch (pixels).
const MINERAL_DEPOT_RADIUS: i32 = 10 * 32;

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
    } else {
        game.draw_text(CoordinateType::Screen, 10, 74, "pool: built");
    }

    supply_overlords(cx, bot, &counts);
}

/// Morphs an overlord when free supply approaches zero: the margin is half
/// an overlord's capacity, so the morph lands before the block.
fn supply_overlords<'game>(
    cx: Context<'_, 'game>,
    bot: &mut ZzzkBot<'game>,
    counts: &crate::util::OwnCounts,
) {
    let game = cx.game();
    let me = game.self_player().expect("the bot plays a match, not a replay");
    let free = me.supply_total() - counts.supply_used;
    let margin = UnitType::ZERG_OVERLORD.supply_provided() / 2;
    if free > margin {
        return;
    }
    // One overlord under way is enough: `incomplete` counts egg payloads too.
    if counts.incomplete.get(&UnitType::ZERG_OVERLORD).copied().unwrap_or(0) > 0 {
        return;
    }

    if let Some(larva) = bot
        .own_units
        .iter(cx)
        .find(|u| u.exists() && u.is_idle() && u.unit_type() == UnitType::ZERG_LARVA)
    {
        if larva.morph(UnitType::ZERG_OVERLORD) {
            game.send_text("morphing overlord");
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
