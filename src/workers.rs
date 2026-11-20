//! Worker and depot orders: demo mining until the full gathering discipline.

use crate::scouting::loc_center;
use crate::ZzzkBot;
use bwapi::{Context, CoordinateType, UnitType};

/// The macro layer: issues build orders by the current strategy.
pub fn macro_orders<'game>(cx: Context<'_, 'game>, bot: &mut ZzzkBot<'game>) {
    // 4-pool: the pool is the first and only concern.
    let counts = crate::util::own_counts(&bot.own_units, cx);
    let have_pool =
        counts.all.get(&UnitType::ZERG_SPAWNING_POOL).copied().unwrap_or(0) > 0;
    if !have_pool {
        let pool = UnitType::ZERG_SPAWNING_POOL;
        let status = match bot.build.plan(cx, &bot.own_units, &bot.scouting, pool) {
            Some(plan) => {
                // Pool is the top priority: the builder goes right now.
                if !plan.builder.build(plan.unit_type, plan.tile) {
                    bot.build.mark_fail(pool, cx.game().frame_count());
                }
                format!("pool: builder #{}, loc {},{}", plan.builder.id(), plan.tile.x, plan.tile.y)
            }
            None => String::from("pool: waiting (minerals, throttle, builder or loc)"),
        };
        cx.game().draw_text(CoordinateType::Screen, 10, 74, &status);
    } else {
        cx.game().draw_text(CoordinateType::Screen, 10, 74, "pool: built");
    }
}

impl<'game> ZzzkBot<'game> {
    /// Idle workers mine a home-base mineral; depots train workers.
    pub fn give_orders(&self, cx: Context<'_, 'game>) {
        for unit in self.own_units.iter(cx) {
            let unit_type = unit.unit_type();
            if unit_type.is_worker() {
                if !unit.is_idle() {
                    continue;
                }
                if unit.is_carrying_gas() || unit.is_carrying_minerals() {
                    unit.return_cargo();
                    continue;
                }
                // Only the home base's crystals: a globally nearest mineral
                // makes a worker released mid-map mine the enemy's patch.
                let home = loc_center(self.scouting.my_start);
                let closest = self
                    .minerals
                    .iter(cx)
                    .filter(|&mineral| {
                        let pos = mineral.position();
                        (pos.x - home.x).abs() + (pos.y - home.y).abs() < 25 * 32
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
