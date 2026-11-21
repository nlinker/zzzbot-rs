//! Scouting: identify the enemy race and start without sacrificing scouts.

use std::collections::HashSet;

use bwapi::{
    Color, Context, CoordinateType, Player, Position, Race, TextColor, TilePosition, Unit,
    UnitSet, UnitType, WeaponType,
};

const MIN_DRONES: i32 = 4;
const ARRIVAL_RADIUS: i32 = 4 * 32;
const DANGER_RADIUS: i32 = 8 * 32;
const START_RADIUS: i32 = 12 * 32;
/// How close the army must stand to a guessed start to test it, and how far
/// from it an enemy unit confirms the guess.
const VERIFY_RADIUS: i32 = 12 * 32;
/// Army types that travel to the enemy start and thus can verify a guess.
const ARMY_CHECK: [UnitType; 4] = [
    UnitType::ZERG_ZERGLING,
    UnitType::ZERG_MUTALISK,
    UnitType::ZERG_HYDRALISK,
    UnitType::ZERG_ULTRALISK,
];

pub fn loc_center(loc: TilePosition) -> Position {
    Position::new(loc.x * 32 + 64, loc.y * 32 + 48)
}

fn walk_dist(a: Position, b: Position) -> i32 {
    (a.x - b.x).abs() + (a.y - b.y).abs()
}

fn threatens(unit: Unit<'_>, air: bool) -> bool {
    let kind = unit.unit_type();
    let weapon = if air { kind.air_weapon() } else { kind.ground_weapon() };
    weapon != WeaponType::NONE && weapon != WeaponType::UNKNOWN
}

pub struct Scouting<'game> {
    pub start_locations: Vec<TilePosition>,
    pub my_start: TilePosition,
    /// Candidate enemy starts not yet ruled out.
    pub unscouted: HashSet<TilePosition>,
    /// Enemy starts the scout has walked up to.
    pub scouted: HashSet<TilePosition>,
    /// The enemy player (first of them); the race may still read Unknown/Random.
    pub enemy: Option<Player<'game>>,
    /// The enemy race, deduced from a seen unit; unknown until first contact.
    pub enemy_race: Option<Race>,
    /// The enemy start: seen buildings or the last-unscouted deduction.
    pub enemy_start: Option<TilePosition>,
    /// `enemy_start` is a seen depot (hard evidence), not a deduction; the
    /// scouts keep working while the start is only guessed.
    pub pinned_start: bool,
    /// A guessed start our army found empty, for the match log.
    pub guess_invalidated: Option<TilePosition>,
    /// The drone walking the start locations; released back to mining when done.
    scout: Option<Unit<'game>>,
    /// Where the scout is headed right now.
    target: Option<TilePosition>,
    /// The overlord flying the start locations; parks home when done.
    overlord: Option<Unit<'game>>,
    ovl_target: Option<TilePosition>,
    /// Starts with hostile troops: don't send the drone there again.
    unsafe_starts: HashSet<TilePosition>,
    drone_retreating: bool,
    /// The initial scouting overlord must never be sent back after first contact.
    overlord_retired: bool,
    /// All-in opener: the drone scout must not leave before the pool order —
    /// four mining drones reach the 200 minerals measurably faster.
    hold_drone_until_pool: bool,
    /// One-shot drone scout (all-in): check the single nearest candidate,
    /// then back to mining — the overlord and the lings cover the rest.
    scout_oneshot: bool,
    oneshot_picked: bool,
    /// Set on any enemy_start change: the army re-aims its marchers once.
    pub retarget_fresh: bool,
}

impl<'game> Scouting<'game> {
    pub fn new(cx: Context<'_, 'game>, my_start: TilePosition, all_in: bool) -> Self {
        let game = cx.game();
        let start_locations = game.start_locations();
        let unscouted = start_locations.iter().copied()
            .filter(|&loc| loc != my_start).collect();
        let enemy = game.enemies().iter(cx).next();
        Self {
            start_locations,
            my_start,
            unscouted,
            scouted: HashSet::new(),
            enemy,
            enemy_race: None,
            enemy_start: None,
            pinned_start: false,
            guess_invalidated: None,
            scout: None,
            target: None,
            overlord: None,
            ovl_target: None,
            unsafe_starts: HashSet::new(),
            drone_retreating: false,
            overlord_retired: false,
            hold_drone_until_pool: all_in,
            scout_oneshot: all_in,
            oneshot_picked: false,
            retarget_fresh: false,
        }
    }

    /// Runs both scouts: the drone walks the starts, the overlord flies them.
    /// Both stop once the enemy race and start are known.
    pub fn update(&mut self, cx: Context<'_, 'game>, own: &UnitSet<'game>) {
        // Only a pinned start (or nothing left to check) retires the scouts:
        // a deduction often points at the wrong start, and then the army
        // would march there forever while the enemy builds elsewhere.
        if self.enemy_race.is_some()
            && (self.pinned_start || (self.enemy_start.is_some() && self.unscouted.is_empty()))
        {
            self.release();
            self.park_overlord();
            return;
        }

        // Check threats every frame, not only when on_unit_show fires; collect
        // the visible depots to tell occupied starts from cleared ones.
        let mut enemy_depots: Vec<Position> = Vec::new();
        if let Some(enemy) = self.enemy {
            for unit in cx.game().all_units().iter(cx) {
                if unit.exists() && unit.is_visible() && unit.player() == enemy {
                    if unit.unit_type().is_resource_depot()
                        && !unit.unit_type().is_flying_building()
                    {
                        enemy_depots.push(unit.position());
                    }
                    self.observe_enemy(unit);
                }
            }
        }

        // A start is cleared only when its building area can actually be
        // observed: merely moving within four tiles is not proof of an empty
        // start, and tiles visible under an enemy depot mean occupied.
        for &loc in &self.start_locations {
            if loc == self.my_start || self.unsafe_starts.contains(&loc)
                || self.scouted.contains(&loc)
            {
                continue;
            }
            let fully_visible = (0..4).all(|dx| (0..3).all(|dy| {
                cx.game().is_visible(TilePosition::new(loc.x + dx, loc.y + dy))
            }));
            let occupied = enemy_depots.iter()
                .any(|&pos| walk_dist(loc_center(loc), pos) < START_RADIUS);
            if fully_visible && !occupied && self.unscouted.remove(&loc) {
                self.scouted.insert(loc);
            }
        }

        if self.enemy_start.is_none() && self.unscouted.len() == 1 {
            self.enemy_start = self.unscouted.iter().next().copied();
            self.pinned_start = false;
            self.retarget_fresh = true;
        }

        // A guessed start must confirm itself: our standing army in an empty
        // location refutes the guess — clear it and let the deduction pick
        // the next candidate. A pinned start is never refuted this way.
        if !self.pinned_start {
            if let Some(loc) = self.enemy_start {
                let center = loc_center(loc);
                let army_near = own.iter(cx).any(|u| {
                    ARMY_CHECK.contains(&u.unit_type())
                        && u.exists()
                        && u.is_completed()
                        && u.distance_to_position(center) < VERIFY_RADIUS
                });
                let enemy_near = self.enemy.is_some_and(|enemy| {
                    cx.game().units_in_radius(center, VERIFY_RADIUS)
                        .any(|u| u.exists() && u.player() == enemy)
                });
                if army_near && !enemy_near {
                    self.guess_invalidated = Some(loc);
                    self.enemy_start = None;
                    if self.unscouted.remove(&loc) {
                        self.scouted.insert(loc);
                    }
                }
            }
        }

        // All-in: hold the drone scout until the pool is placed — mining
        // reaches 200 faster with four drones; the overlord covers the sky.
        let pool_exists = own.iter(cx).any(|u| {
            u.exists() && u.unit_type() == UnitType::ZERG_SPAWNING_POOL
        });
        if !self.hold_drone_until_pool || pool_exists {
            self.update_drone(cx, own);
        }
        self.update_overlord(cx, own);
    }

    /// Scouts unexplored start locations with a drone, returning it when done or short on drones.
    fn update_drone(&mut self, cx: Context<'_, 'game>, own: &UnitSet<'game>) {
        if self.enemy_start.is_some() && self.enemy_race.is_some() {
            self.release();
            return;
        }
        if let Some(scout) = self.scout {
            if !scout.exists() {
                self.scout = None;
                self.target = None;
                self.drone_retreating = false;
            }
        }

        let drone_count = own.iter(cx).filter(|u| {
            u.exists() && u.is_completed() && !u.is_morphing()
                && u.unit_type() == UnitType::ZERG_DRONE
        }).count() as i32;
        if drone_count < MIN_DRONES {
            self.release();
            return;
        }

        if self.scout.is_none() {
            // Do not repeatedly take workers away from mining when no safe target remains.
            if self.nearest_unscouted(loc_center(self.my_start), None).is_none() {
                return;
            }
            let base = loc_center(self.my_start);
            self.scout = own.iter(cx)
                .filter(|u| u.exists() && u.is_completed() && !u.is_morphing()
                    && u.unit_type() == UnitType::ZERG_DRONE)
                .min_by_key(|u| u.distance_to_position(base));
        }
        let Some(scout) = self.scout else { return };

        if self.drone_retreating {
            if scout.distance_to_position(loc_center(self.my_start)) > ARRIVAL_RADIUS {
                if scout.is_idle() {
                    let _ = scout.move_to(loc_center(self.my_start));
                }
                return;
            }
            self.drone_retreating = false;
        }

        if self.target.is_some_and(|loc| !self.unscouted.contains(&loc)
            || self.unsafe_starts.contains(&loc)) {
            self.target = None;
        }
        // One-shot scout: the first chosen location is the only one — once
        // it resolves (cleared, occupied or unsafe) the drone goes home to
        // mine instead of walking the rest of the map.
        if self.scout_oneshot {
            if self.target.is_some() {
                self.oneshot_picked = true;
            } else if self.oneshot_picked {
                self.release();
                return;
            }
        }
        if self.target.is_none() {
            self.target = self.nearest_unscouted(scout.position(), self.ovl_target);
            if let Some(target) = self.target {
                let _ = scout.move_to(loc_center(target));
            } else {
                self.release();
            }
            return;
        }
        let target = self.target.unwrap();
        if scout.is_idle() {
            let _ = scout.move_to(loc_center(target));
        }
    }

    /// The overlord flies the starts the drone is not walking to.
    fn update_overlord(&mut self, cx: Context<'_, 'game>, own: &UnitSet<'game>) {
        if self.overlord_retired {
            return;
        }
        if self.overlord.is_some_and(|unit| !unit.exists()) {
            self.overlord = None;
            self.ovl_target = None;
        }
        if self.overlord.is_none() {
            self.overlord = own.iter(cx).find(|u| {
                u.exists() && u.is_completed() && u.unit_type() == UnitType::ZERG_OVERLORD
            });
        }
        let Some(overlord) = self.overlord else { return };
        if self.ovl_target.is_some_and(|loc| !self.unscouted.contains(&loc)) {
            self.ovl_target = None;
        }
        if self.ovl_target.is_none() {
            self.ovl_target = self.nearest_unscouted(overlord.position(), self.target);
            if let Some(target) = self.ovl_target {
                let _ = overlord.move_to(loc_center(target));
            } else {
                self.park_overlord();
                self.overlord_retired = true;
            }
        } else if overlord.is_idle() {
            let _ = overlord.move_to(loc_center(self.ovl_target.unwrap()));
        }
    }

    /// Is this unit one of the scouts.
    pub fn is_scout(&self, unit: Unit<'game>) -> bool {
        self.scout == Some(unit) || self.overlord == Some(unit)
    }

    /// Called by on_unit_show. Returns race only when learned for the first time.
    pub fn note_enemy_unit(&mut self, unit: Unit<'game>) -> Option<Race> {
        if Some(unit.player()) != self.enemy {
            return None;
        }
        let first_race = match unit.unit_type().race() {
            race @ (Race::ZERG | Race::TERRAN | Race::PROTOSS)
                if self.enemy_race.is_none() => {
                self.enemy_race = Some(race);
                Some(race)
            }
            _ => None,
        };
        self.observe_enemy(unit);
        first_race
    }

    fn observe_enemy(&mut self, unit: Unit<'game>) {
        if self.enemy_race.is_none() {
            match unit.unit_type().race() {
                race @ (Race::ZERG | Race::TERRAN | Race::PROTOSS) => {
                    self.enemy_race = Some(race);
                }
                _ => {}
            }
        }

        let position = unit.position();
        // Any non-flying enemy near a start pins it: a worker, army or depot
        // all mean the enemy lives there — for the rush only the WHERE
        // matters, and waiting for a depot wastes the turn it takes to spot
        // one.
        if !unit.is_flying() {
            let pinned = self.start_locations.iter().copied().find(|&loc| {
                loc != self.my_start && walk_dist(loc_center(loc), position) < START_RADIUS
            });
            if pinned.is_some() && (pinned != self.enemy_start || !self.pinned_start) {
                self.enemy_start = pinned;
                self.pinned_start = true;
                self.retarget_fresh = true;
            }
        }

        if self.overlord.is_some_and(|ovl| ovl.exists()
            && walk_dist(ovl.position(), position) < DANGER_RADIUS) {
            // Any enemy contact ends overlord scouting, including non-AA units.
            self.park_overlord();
            self.overlord_retired = true;
        }

        if let Some(drone) = self.scout {
            if drone.exists() && threatens(unit, false)
                && walk_dist(drone.position(), position) < DANGER_RADIUS {
                // Do not cross a suspected defended start again.
                if let Some(target) = self.target {
                    self.unsafe_starts.insert(target);
                }
                self.target = None;
                if !self.drone_retreating {
                    self.drone_retreating = true;
                    let _ = drone.move_to(loc_center(self.my_start));
                }
            }
        }
    }

    fn nearest_unscouted(&self, from: Position, exclude: Option<TilePosition>) -> Option<TilePosition> {
        self.unscouted.iter().copied()
            .filter(|loc| Some(*loc) != exclude && !self.unsafe_starts.contains(loc))
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
        self.drone_retreating = false;
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
            } else if self.unsafe_starts.contains(&loc) {
                (Color::ORANGE, "danger")
            } else {
                (Color::YELLOW, "?")
            };
            let left_top = Position::new(loc.x * 32, loc.y * 32);
            let right_bottom = Position::new((loc.x + 4) * 32, (loc.y + 3) * 32);
            game.draw_box_map(left_top, right_bottom, color);
            game.draw_text_map(left_top, label);
        }
        if let Some(scout) = self.scout {
            if scout.exists() { game.draw_circle_map(scout.position(), 24, Color::ORANGE); }
        }
        if let Some(overlord) = self.overlord {
            if overlord.exists() { game.draw_circle_map(overlord.position(), 24, Color::BLUE); }
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
            TextColor::Green, self.start_locations.len(), self.unscouted.len(),
            enemy, TextColor::Default,
        );
        game.draw_text(CoordinateType::Screen, 10, 58, &line);
    }
}
