//! Worker and depot orders: demo mining until the full gathering discipline.

use crate::ZzzkBot;
use crate::scouting::loc_center;
use bwapi::Context;

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
