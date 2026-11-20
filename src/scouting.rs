//! Scouting: start locations, enemy identification, the scout drone.

use std::collections::HashSet;

use bwapi::{
    Color, Context, CoordinateType, Player, Position, TextColor, TilePosition, Unit, UnitSet,
    UnitType,
};

/// Below this many completed drones the scout walks home and mines instead.
const MIN_DRONES: i32 = 4;

/// Rough center of a start location in pixels.
fn loc_center(loc: TilePosition) -> Position {
    Position::new(loc.x * 32 + 64, loc.y * 32 + 48)
}

fn walk_dist(a: Position, b: Position) -> i32 {
    (a.x - b.x).abs() + (a.y - b.y).abs()
}

pub struct Scouting<'game> {
    /// Every start location on the map, including mine.
    pub start_locations: Vec<TilePosition>,
    pub my_start: TilePosition,
    /// Enemy starts not yet looked at; the scout clears them one by one.
    pub unscouted: HashSet<TilePosition>,
    /// Enemy starts the scout has walked up to.
    pub scouted: HashSet<TilePosition>,
    /// The enemy player (first of them); the race may still read Unknown/Random.
    pub enemy: Option<Player<'game>>,
    /// The drone walking the start locations; released back to mining when done.
    scout: Option<Unit<'game>>,
    /// Where the scout is headed right now.
    target: Option<TilePosition>,
}

impl<'game> Scouting<'game> {
    pub fn new(cx: Context<'_, 'game>, my_start: TilePosition) -> Self {
        let game = cx.game();
        let start_locations = game.start_locations();
        let unscouted =
            start_locations.iter().copied().filter(|&loc| loc != my_start).collect();
        let enemy = game.enemies().iter(cx).next();
        Scouting {
            start_locations,
            my_start,
            unscouted,
            scouted: HashSet::new(),
            enemy,
            scout: None,
            target: None,
        }
    }

    /// Scouts unexplored start locations with a drone, returning it when done or short on drones.
    pub fn update(&mut self, cx: Context<'_, 'game>, own: &UnitSet<'game>) {
        if let Some(scout) = self.scout {
            if !scout.exists() {
                self.release();
            }
        }

        let drone_count = own
            .iter(cx)
            .filter(|u| {
                u.exists() && u.is_completed() && !u.is_morphing()
                    && u.unit_type() == UnitType::ZERG_DRONE
            })
            .count() as i32;
        if self.unscouted.is_empty() || drone_count < MIN_DRONES {
            self.release();
            return;
        }

        if self.scout.is_none() {
            // Nearest drone to the base, so mining lines stay short.
            let base = loc_center(self.my_start);
            self.scout = own
                .iter(cx)
                .filter(|u| {
                    u.exists() && u.is_completed() && !u.is_morphing()
                        && u.unit_type() == UnitType::ZERG_DRONE
                })
                .min_by_key(|u| u.distance_to_position(base));
        }
        let Some(scout) = self.scout else { return };

        if self.target.is_none() {
            self.target = self.nearest_unscouted(scout.position());
            if let Some(target) = self.target {
                // Move the drone immediately, since mining drones never become idle.
                let _ = scout.move_to(loc_center(target));
            }
            return;
        }
        let Some(target) = self.target else { return };

        let center = loc_center(target);
        if scout.distance_to_position(center) < 4 * 32 {
            // Arrived: mark it and head for the next one.
            self.unscouted.remove(&target);
            self.scouted.insert(target);
            self.target = self.nearest_unscouted(center);
            if let Some(next) = self.target {
                let _ = scout.move_to(loc_center(next));
            }
        } else if scout.is_idle() {
            // Reissue in case something interrupted the walk.
            let _ = scout.move_to(center);
        }
    }

    fn nearest_unscouted(&self, from: Position) -> Option<TilePosition> {
        self.unscouted
            .iter()
            .copied()
            .min_by_key(|&loc| walk_dist(loc_center(loc), from))
    }

    /// Frees the scout; `workers::give_orders` reassigns it once it idles.
    fn release(&mut self) {
        self.scout = None;
        self.target = None;
    }

    /// Draws a box per start location on the map and the enemy line on screen.
    pub fn draw(&self, cx: Context<'_, 'game>) {
        let game = cx.game();
        for &loc in &self.start_locations {
            let (color, label) = if loc == self.my_start {
                (Color::GREEN, "mine")
            } else if self.scouted.contains(&loc) {
                (Color::CYAN, "ok")
            } else {
                (Color::YELLOW, "?")
            };
            let left_top = Position::new(loc.x * 32, loc.y * 32);
            let right_bottom = Position::new((loc.x + 4) * 32, (loc.y + 3) * 32);
            game.draw_box_map(left_top, right_bottom, color);
            game.draw_text_map(left_top, label);
        }
        if let Some(scout) = self.scout {
            game.draw_circle_map(scout.position(), 24, Color::ORANGE);
        }

        let enemy = match self.enemy {
            Some(enemy) => format!("{} ({})", enemy.name(), enemy.race().name()),
            None => String::from("none"),
        };
        let line = format!(
            "{}Starts {}, not scouted {}, enemy {}{}",
            TextColor::Green,
            self.start_locations.len(),
            self.unscouted.len(),
            enemy,
            TextColor::Default,
        );
        game.draw_text(CoordinateType::Screen, 10, 58, &line);
    }
}
