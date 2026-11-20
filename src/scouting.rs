//! Scouting: start locations, enemy identification, the unscouted set.

use std::collections::HashSet;

use bwapi::{Color, CoordinateType, Player, Position, TextColor, TilePosition};

pub struct Scouting<'game> {
    /// Every start location on the map, including mine.
    pub start_locations: Vec<TilePosition>,
    pub my_start: TilePosition,
    /// Enemy starts not yet looked at; the scout clears them one by one.
    pub unscouted: HashSet<TilePosition>,
    /// The enemy player (first of them); the race may still read Unknown/Random.
    pub enemy: Option<Player<'game>>,
}

impl<'game> Scouting<'game> {
    pub fn new(cx: bwapi::Context<'_, 'game>, my_start: TilePosition) -> Self {
        let game = cx.game();
        let start_locations = game.start_locations();
        let unscouted =
            start_locations.iter().copied().filter(|&loc| loc != my_start).collect();
        let enemy = game.enemies().iter(cx).next();
        Scouting { start_locations, my_start, unscouted, enemy }
    }

    /// Draws a box per start location on the map and the enemy line on screen.
    pub fn draw(&self, cx: bwapi::Context<'_, 'game>) {
        let game = cx.game();
        for &loc in &self.start_locations {
            let mine = loc == self.my_start;
            let color = if mine { Color::GREEN } else { Color::YELLOW };
            let left_top = Position::new(loc.x * 32, loc.y * 32);
            let right_bottom = Position::new((loc.x + 4) * 32, (loc.y + 3) * 32);
            game.draw_box_map(left_top, right_bottom, color);
            game.draw_text_map(left_top, if mine { "mine" } else { "?" });
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
