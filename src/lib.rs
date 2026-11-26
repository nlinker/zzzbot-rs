//! The ZZZBot-rs bot: a StarCraft: Brood War AI module built on bwapi-rs.
//! The strategy is async tasks on `reactor`.
mod log;
mod reactor;
mod tasks;

use std::rc::Rc;

use bwapi::{Bot, Color, Context, CoordinateType, TextColor, UnitSet};
use log::Logger;
use reactor::Reactor;
use tasks::heartbeat;

/// Game speed from the first frame on.
const LOCAL_SPEED_START: i32 = 1;

/// The bot of one match: the tasks and what they share.
pub struct ZzzBot<'game> {
    name: String,
    /// Live set descriptor: valid for the whole match, read through a `Context`.
    own_units: UnitSet<'game>,
    /// Decision-point log, truncated at match start. Shared with the tasks.
    logger: Rc<Logger>,
    /// The async tasks, polled once per frame.
    reactor: Reactor<'game>,
}

impl<'game> ZzzBot<'game> {
    /// Overlay: frame, units, resources.
    fn draw_stats(&self, cx: Context<'_, 'game>) {
        let game = cx.game();
        let message = format!(
            "{}Frame {} (build {}){}, own units {}",
            TextColor::Green,
            game.frame_count(),
            env!("BUILD_UNIX_TIME"),
            TextColor::Default,
            self.own_units.len(cx),
        );
        game.draw_text(CoordinateType::Screen, 10, 10, &message);
        game.draw_line(CoordinateType::Screen, 10, 21, 40, 21, Color::RED);

        // BWAPI counts supply in halves.
        let me = game.self_player().expect("the bot plays a match, not a replay");
        let supply = format!(
            "U {}/{}, m {} g {}",
            me.supply_used() / 2,
            me.supply_total() / 2,
            me.minerals(),
            me.gas(),
        );
        game.draw_text(CoordinateType::Screen, 10, 26, &supply);
    }
}

impl<'game> Bot<'game> for ZzzBot<'game> {
    fn on_start(cx: Context<'_, 'game>) -> Self {
        let game = cx.game();
        let name = String::from("ZZZBot-rs");
        game.send_text(&format!("Hello from Rust! {name} reporting in"));

        // Fast-forward, single player only.
        game.set_local_speed(LOCAL_SPEED_START);

        let me = game.self_player().expect("the bot plays a match, not a replay");
        let logger = Rc::new(Logger::open());
        logger.line(
            game.frame_count(),
            &format!("start: build {}, map {}", env!("BUILD_UNIX_TIME"), game.map_name()),
        );

        let mut reactor = Reactor::new();
        reactor.spawn(heartbeat(game, logger.clone()));

        ZzzBot { name, own_units: me.units(), logger, reactor }
    }

    fn on_end(self, cx: Context<'_, 'game>, is_winner: bool) {
        let game = cx.game();
        let result = if is_winner { "won" } else { "lost" };
        self.logger.line(
            game.frame_count(),
            &format!("end: {result}, own units {}", self.own_units.len(cx)),
        );
        game.send_text(&format!("{} {result} at frame {}", self.name, game.frame_count()));
    }

    fn on_frame(&mut self, cx: Context<'_, 'game>) {
        self.draw_stats(cx);
        self.reactor.tick();
    }
}

bwapi::bot!(ZzzBot);
