//! Building placement and orders: the generic "build X" engine.

use std::collections::HashMap;

use crate::scouting::Scouting;
use bwapi::{Context, Position, TilePosition, Unit, UnitSet, UnitType};

/// Frames between retries after a failed placement.
const RETRY_FRAMES: i32 = 24;

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

    pub fn is_builder(&self, unit: Unit<'game>) -> bool {
        self.builders.values().any(|&builder| builder == unit)
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
        if let Some(&failed_at) = self.last_fail.get(&unit_type) {
            if game.frame_count() < failed_at + RETRY_FRAMES {
                return None;
            }
        }

        // Already on it: the reserved builder walks there or morphs.
        if let Some(&builder) = self.builders.get(&unit_type) {
            if builder.exists() && builder.unit_type() == UnitType::ZERG_DRONE {
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
