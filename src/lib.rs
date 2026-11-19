//! The ZZZKBot-rs bot: a StarCraft: Brood War AI module built on bwapi-rs.

use bwapi::{Bot, Color, Context, CoordinateType, Game, TextColor, Unit, UnitQuery, UnitSet};

pub struct ZzzkBot<'game> {
    name: String,
    /// Live set descriptors: valid for the whole match, read through a `Context`.
    own_units: UnitSet<'game>,
    minerals: UnitSet<'game>,
    /// Unit handles, valid for the whole match.
    created: Vec<Unit<'game>>,
    /// An owned query result, read a few items at a time across frames.
    around_base: Option<UnitQuery<'game>>,
    scanned: usize,
}

impl<'game> ZzzkBot<'game> {
    fn draw_stats(&self, cx: Context<'_, 'game>) {
        let game = cx.game();
        let message = format!(
            "{}Frame {}{}, own units {}, created {}, scanned {}",
            TextColor::Green,
            game.frame_count(),
            TextColor::Default,
            self.own_units.len(cx),
            self.created.len(),
            self.scanned,
        );
        game.draw_text(CoordinateType::Screen, 10, 10, &message);
        game.draw_line(CoordinateType::Screen, 10, 21, 40, 21, Color::RED);
    }

    fn give_orders(&self, cx: Context<'_, 'game>) {
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

    /// Marks one unit around the base per frame; the query keeps its position
    /// between frames.
    fn scan_base(&mut self, game: Game<'game>) {
        let Some(unit) = self.around_base.as_mut().and_then(Iterator::next) else {
            return;
        };
        self.scanned += 1;
        game.draw_circle_map(unit.position(), 16, Color::YELLOW);
    }
}

impl<'game> Bot<'game> for ZzzkBot<'game> {
    fn on_start(cx: Context<'_, 'game>) -> Self {
        let game = cx.game();
        let name = String::from("ZZZKBot-rs");
        game.send_text(&format!("Hello from Rust! {name} reporting in"));

        let me = game
            .self_player()
            .expect("the bot plays a match, not a replay");
        let own_units = me.units();
        let base = own_units
            .iter(cx)
            .find(|u| u.unit_type().is_resource_depot());
        ZzzkBot {
            name,
            own_units,
            minerals: game.minerals(),
            created: Vec::new(),
            around_base: base.map(|depot| game.units_in_radius(depot.position(), 320)),
            scanned: 0,
        }
    }

    fn on_frame(&mut self, cx: Context<'_, 'game>) {
        self.draw_stats(cx);
        self.give_orders(cx);
        self.scan_base(cx.game());
    }

    fn on_unit_create(&mut self, _cx: Context<'_, 'game>, unit: Unit<'game>) {
        self.created.push(unit);
    }

    fn on_end(self, cx: Context<'_, 'game>, is_winner: bool) {
        let game = cx.game();
        let result = if is_winner { "won" } else { "lost" };
        game.send_text(&format!(
            "{} {result} at frame {}",
            self.name,
            game.frame_count()
        ));
    }
}

bwapi::bot!(ZzzkBot);
