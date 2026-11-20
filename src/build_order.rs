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
pub struct BuildEngine {
    /// Frame of the last failed attempt, per building type.
    last_fail: HashMap<UnitType, i32>,
}

impl BuildEngine {
    pub fn new() -> Self {
        BuildEngine::default()
    }

    /// Picks a builder and tile when retry allows. The caller builds.
    pub fn plan<'game>(
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

        let base = scouting.my_start;
        let builder = pick_builder(cx, own, scouting, Position::new(base.x * 32, base.y * 32))?;

        let tile = game.build_location_with(unit_type, base, 64, unit_type.requires_creep());
        if !game.can_build_here_with(tile, unit_type, Some(builder), false) {
            self.last_fail.insert(unit_type, game.frame_count());
            return None;
        }
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
