//! The ZZZKBot-rs bot: a StarCraft: Brood War AI module built on bwapi-rs.

mod strategy;
mod util;
mod workers;

use bwapi::{Bot, Color, Context, CoordinateType, Game, TextColor, Unit, UnitQuery, UnitSet, UnitType};
use strategy::Strategy;

pub struct ZzzkBot<'game> {
    name: String,
    /// Live set descriptors: valid for the whole match, read through a `Context`.
    pub own_units: UnitSet<'game>,
    pub minerals: UnitSet<'game>,
    pub strategy: Strategy,
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

        let counts = util::own_counts(&self.own_units, cx);
        let me = game.self_player().expect("the bot plays a match, not a replay");
        let lings = counts.all.get(&UnitType::ZERG_ZERGLING).copied().unwrap_or(0);
        let lings_done = counts.completed.get(&UnitType::ZERG_ZERGLING).copied().unwrap_or(0);
        let supply = format!(
            "U {}/{}, lings {} (+{} in eggs)",
            util::supply_str(counts.supply_used),
            util::supply_str(me.supply_total()),
            lings_done,
            lings - lings_done,
        );
        game.draw_text(CoordinateType::Screen, 10, 26, &supply);

        let order = match self.strategy.order {
            Some(order) => format!("{}, sunkens {}", order.name(), self.strategy.num_sunkens),
            None => String::from("standard, pending scout"),
        };
        game.draw_text(CoordinateType::Screen, 10, 42, &format!("BO {order}"));
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

        // Fast-forward (single player only); raise the skip for even more speed.
        game.set_local_speed(0);
        //game.set_frame_skip(2);

        let me = game.self_player().expect("the bot plays a match, not a replay");
        let own_units = me.units();
        let start = me.start_location();

        // Strategy seed: same map and start tend to play out the same way.
        let mut seed = 0xcbf2_9ce4_8422_2325u64;
        for byte in game.map_hash().as_bytes() {
            seed = (seed ^ *byte as u64).wrapping_mul(0x100_0000_01b3);
        }
        seed ^= (start.x as u64) << 32 ^ start.y as u64;

        let base = own_units.iter(cx).find(|u| u.unit_type().is_resource_depot());
        ZzzkBot {
            name,
            own_units,
            minerals: game.minerals(),
            strategy: Strategy::new(seed),
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
        game.send_text(&format!("{} {result} at frame {}", self.name, game.frame_count()));
    }
}

bwapi::bot!(ZzzkBot);
