//! Worker and depot orders: demo mining until the full gathering discipline.

use crate::ZzzkBot;
use bwapi::Context;

impl<'game> ZzzkBot<'game> {
    /// Idle workers mine the nearest mineral; depots train workers.
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
                let closest = self
                    .minerals
                    .iter(cx)
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
