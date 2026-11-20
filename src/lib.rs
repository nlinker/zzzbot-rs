//! The ZZZKBot-rs bot: a StarCraft: Brood War AI module built on bwapi-rs.

mod army;
mod build_order;
mod log;
mod scouting;
mod strategy;
mod util;
mod workers;

use bwapi::{Bot, Color, Context, CoordinateType, Game, TextColor, Unit, UnitQuery, UnitSet, UnitType};
use build_order::BuildEngine;
use log::Logger;
use scouting::Scouting;
use workers::macro_orders;
use strategy::{Opener, Strategy};

const LOCAL_SPEED: i32 = 0;

pub struct ZzzkBot<'game> {
    name: String,
    /// Live set descriptors: valid for the whole match, read through a `Context`.
    pub own_units: UnitSet<'game>,
    pub minerals: UnitSet<'game>,
    pub strategy: Strategy,
    pub scouting: Scouting<'game>,
    pub build: BuildEngine<'game>,
    /// Drones assigned to gas; the rest of the workers stay on minerals.
    pub gas_drones: Vec<Unit<'game>>,
    /// The completed refinery, if any.
    pub refinery: Option<Unit<'game>>,
    /// Extractor trick.
    pub trick_builder: Option<Unit<'game>>,
    pub trick_extractor: Option<Unit<'game>>,
    /// Decision-point log, truncated at match start.
    pub logger: Logger,
    /// Unit handles, valid for the whole match.
    created: Vec<Unit<'game>>,
    /// An owned query result, read a few items at a time across frames.
    around_base: Option<UnitQuery<'game>>,
    scanned: usize,
}

impl<'game> ZzzkBot<'game> {
    /// One log line at a decision point; called sparingly, so the file stays
    /// small.
    pub fn log(&self, cx: Context<'_, 'game>, message: &str) {
        self.logger.line(cx.game().frame_count(), message);
    }

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
        let mutas = counts.all.get(&UnitType::ZERG_MUTALISK).copied().unwrap_or(0);
        let mutas_done = counts.completed.get(&UnitType::ZERG_MUTALISK).copied().unwrap_or(0);
        let hydras = counts.all.get(&UnitType::ZERG_HYDRALISK).copied().unwrap_or(0);
        let hydras_done = counts.completed.get(&UnitType::ZERG_HYDRALISK).copied().unwrap_or(0);
        let ultras = counts.all.get(&UnitType::ZERG_ULTRALISK).copied().unwrap_or(0);
        let ultras_done = counts.completed.get(&UnitType::ZERG_ULTRALISK).copied().unwrap_or(0);
        let supply = format!(
            "U {}/{}, lings {} (+{} in eggs), mutas {} (+{}), hydras {} (+{}), ultras {} (+{})",
            util::supply_str(counts.supply_used),
            util::supply_str(me.supply_total()),
            lings_done,
            lings - lings_done,
            mutas_done,
            mutas - mutas_done,
            hydras_done,
            hydras - hydras_done,
            ultras_done,
            ultras - ultras_done,
        );
        game.draw_text(CoordinateType::Screen, 10, 26, &supply);

        let order = match self.strategy.order {
            Some(order) => format!("{}, sunkens {}", order.name(), self.strategy.num_sunkens),
            None => String::from("awaiting scout"),
        };
        let opener = match self.strategy.opener {
            Opener::Pool4 => "4pool",
            Opener::Standard => "standard",
        };
        game.draw_text(
            CoordinateType::Screen,
            10,
            42,
            &format!("opener {opener} | BO {order}"),
        );
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
        game.set_local_speed(LOCAL_SPEED);
        // game.set_frame_skip(2);

        let me = game.self_player().expect("the bot plays a match, not a replay");
        let own_units = me.units();
        let start = me.start_location();

        // Strategy seed: map and start keep runs on one map comparable; the
        // wall-clock adds entropy so restarts reroll the opener.
        let mut seed = 0xcbf2_9ce4_8422_2325u64;
        for byte in game.map_hash().as_bytes() {
            seed = (seed ^ *byte as u64).wrapping_mul(0x100_0000_01b3);
        }
        seed ^= (start.x as u64) << 32 ^ start.y as u64;
        seed ^= std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0);

        let base = own_units.iter(cx).find(|u| u.unit_type().is_resource_depot());
        let strategy = Strategy::new(seed);
        let opener = match strategy.opener {
            Opener::Pool4 => "4pool",
            Opener::Standard => "standard",
        };
        game.send_text(&format!("seed {seed:016x}, opener {opener}"));
        let logger = Logger::open();
        logger.line(
            game.frame_count(),
            &format!("start: opener {opener}, seed {seed:016x}, map {}", game.map_name()),
        );
        ZzzkBot {
            name,
            own_units,
            minerals: game.minerals(),
            strategy,
            scouting: Scouting::new(cx, start),
            build: BuildEngine::new(),
            gas_drones: Vec::new(),
            refinery: None,
            trick_builder: None,
            trick_extractor: None,
            logger,
            created: Vec::new(),
            around_base: base.map(|depot| game.units_in_radius(depot.position(), 320)),
            scanned: 0,
        }
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
        self.scouting.update(cx, &self.own_units);
        self.scouting.draw(cx);
        macro_orders(cx, self);
        self.give_orders(cx);
        self.scan_base(cx.game());
    }

    fn on_unit_create(&mut self, _cx: Context<'_, 'game>, unit: Unit<'game>) {
        self.created.push(unit);
    }

    fn on_unit_show(&mut self, cx: Context<'_, 'game>, unit: Unit<'game>) {
        if let Some(race) = self.scouting.note_enemy_unit(unit) {
            self.strategy.finalize(race);
            let order = self
                .strategy
                .order
                .map_or(String::from("?"), |o| o.name().to_string());
            self.logger.line(
                cx.game().frame_count(),
                &format!(
                    "scouted: race {}, order {order}, sunkens {}",
                    race.name(),
                    self.strategy.num_sunkens
                ),
            );
        }
    }
}

bwapi::bot!(ZzzkBot);
