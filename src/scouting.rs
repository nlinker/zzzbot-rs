//! Scouting: start locations, enemy identification, the scout drone and overlord.

use std::collections::HashSet;

use bwapi::{
    Color, Context, CoordinateType, Player, Position, Race, TextColor, TilePosition, Unit,
    UnitSet, UnitType,
};

/// Below this many completed drones the scout walks home and mines instead.
const MIN_DRONES: i32 = 4;

/// Rough center of a start location in pixels.
pub fn loc_center(loc: TilePosition) -> Position {
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
    /// The enemy race, deduced from a seen unit; unknown until first contact.
    pub enemy_race: Option<Race>,
    /// The enemy start: seen buildings or the last-unscouted deduction.
    pub enemy_start: Option<TilePosition>,
    /// The drone walking the start locations; released back to mining when done.
    scout: Option<Unit<'game>>,
    /// Where the scout is headed right now.
    target: Option<TilePosition>,
    /// The overlord flying the start locations; parks home when done.
    overlord: Option<Unit<'game>>,
    ovl_target: Option<TilePosition>,
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
            enemy_race: None,
            enemy_start: None,
            scout: None,
            target: None,
            overlord: None,
            ovl_target: None,
        }
    }

    /// Runs both scouts: the drone walks the starts, the overlord flies them.
    /// Both stop once the enemy race and start are known.
    pub fn update(&mut self, cx: Context<'_, 'game>, own: &UnitSet<'game>) {
        if self.enemy_race.is_some() && self.enemy_start.is_some() {
            self.release();
            self.park_overlord();
            return;
        }
        self.update_drone(cx, own);
        self.update_overlord(cx, own);

        // One start left unscouted: the enemy must be there.
        if self.enemy_start.is_none() && self.unscouted.len() == 1 {
            self.enemy_start = self.unscouted.iter().next().copied();
        }
    }

    /// Scouts unexplored start locations with a drone, returning it when done or short on drones.
    fn update_drone(&mut self, cx: Context<'_, 'game>, own: &UnitSet<'game>) {
        if self.enemy_start.is_some() {
            self.release();
        }
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
            self.target = self.nearest_unscouted(scout.position(), None);
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
            self.target = self.nearest_unscouted(center, None);
            if let Some(next) = self.target {
                let _ = scout.move_to(loc_center(next));
            }
        } else if scout.is_idle() {
            // Reissue in case something interrupted the walk.
            let _ = scout.move_to(center);
        }
    }

    /// The overlord flies the starts the drone is not walking to.
    fn update_overlord(&mut self, cx: Context<'_, 'game>, own: &UnitSet<'game>) {
        if let Some(overlord) = self.overlord {
            if !overlord.exists() {
                self.overlord = None;
                self.ovl_target = None;
            }
        }
        if self.overlord.is_none() {
            self.overlord = own.iter(cx).find(|u| {
                u.exists() && u.is_completed() && u.unit_type() == UnitType::ZERG_OVERLORD
            });
        }
        let Some(overlord) = self.overlord else { return };

        // Enemy base pinned but race unknown: fly there for first contact.
        if self.enemy_start.is_some() {
            if self.ovl_target != self.enemy_start {
                self.ovl_target = self.enemy_start;
                let _ = overlord.move_to(loc_center(self.enemy_start.unwrap_or(self.my_start)));
            } else if overlord.is_idle() {
                let _ = overlord.move_to(loc_center(self.ovl_target.unwrap_or(self.my_start)));
            }
            return;
        }

        let Some(target) = self.ovl_target else {
            self.ovl_target = self.nearest_unscouted(overlord.position(), self.target);
            if let Some(target) = self.ovl_target {
                let _ = overlord.move_to(loc_center(target));
            }
            return;
        };

        let center = loc_center(target);
        if overlord.distance_to_position(center) < 4 * 32 {
            self.unscouted.remove(&target);
            self.scouted.insert(target);
            self.ovl_target = self.nearest_unscouted(center, self.target);
            match self.ovl_target {
                Some(next) => {
                    let _ = overlord.move_to(loc_center(next));
                }
                // Map walked: park over the home base.
                None => {
                    let _ = overlord.move_to(loc_center(self.my_start));
                }
            }
        } else if overlord.is_idle() {
            let _ = overlord.move_to(center);
        }
    }

    /// Is this unit one of the scouts.
    pub fn is_scout(&self, unit: Unit<'game>) -> bool {
        self.scout == Some(unit) || self.overlord == Some(unit)
    }

    /// Notes a newly seen enemy unit: first contact pins the race, a building
    /// near a start pins the enemy start. Returns the race on first contact.
    pub fn note_enemy_unit(&mut self, unit: Unit<'game>) -> Option<Race> {
        let Some(enemy) = self.enemy else { return None };
        if unit.player() != enemy {
            return None;
        }

        let unit_type = unit.unit_type();
        let first_race = match unit_type.race() {
            race @ (Race::ZERG | Race::TERRAN | Race::PROTOSS)
                if self.enemy_race.is_none() =>
            {
                self.enemy_race = Some(race);
                Some(race)
            }
            _ => None,
        };

        if self.enemy_start.is_none() && unit_type.is_building() {
            let pos = unit.position();
            let near_start = self.start_locations.iter().copied().find(|&loc| {
                loc != self.my_start && walk_dist(loc_center(loc), pos) < 12 * 32
            });
            self.enemy_start = near_start;
        }
        first_race
    }

    /// Nearest unscouted start; `exclude` keeps the two scouts from
    /// chaining the same locations.
    fn nearest_unscouted(
        &self,
        from: Position,
        exclude: Option<TilePosition>,
    ) -> Option<TilePosition> {
        self.unscouted
            .iter()
            .copied()
            .filter(|&loc| Some(loc) != exclude)
            .min_by_key(|&loc| walk_dist(loc_center(loc), from))
    }

    /// Sends the scout home before releasing it to avoid mining enemy minerals.
    /// `workers::give_orders` takes over once it idles at home.
    fn release(&mut self) {
        if let Some(scout) = self.scout {
            if scout.exists() {
                let _ = scout.move_to(loc_center(self.my_start));
            }
        }
        self.scout = None;
        self.target = None;
    }

    /// Sends the overlord home; its scouting is done.
    fn park_overlord(&mut self) {
        if let Some(overlord) = self.overlord {
            if overlord.exists() {
                let _ = overlord.move_to(loc_center(self.my_start));
            }
        }
        self.overlord = None;
        self.ovl_target = None;
    }

    /// Draws a box per start location on the map and the enemy line on screen.
    pub fn draw(&self, cx: Context<'_, 'game>) {
        let game = cx.game();
        for &loc in &self.start_locations {
            let (color, label) = if Some(loc) == self.enemy_start {
                (Color::RED, "enemy")
            } else if loc == self.my_start {
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
        if let Some(overlord) = self.overlord {
            game.draw_circle_map(overlord.position(), 24, Color::BLUE);
        }

        let enemy = match self.enemy {
            Some(enemy) => {
                let race = match self.enemy_race {
                    Some(race) => race.name(),
                    None => enemy.race().name(),
                };
                let start = match self.enemy_start {
                    Some(loc) => format!(" at {},{}", loc.x, loc.y),
                    None => String::from(", start unknown"),
                };
                format!("{} ({}){start}", enemy.name(), race)
            }
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
