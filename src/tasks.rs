//! The strategy as async tasks. A task sees the game through `Game` and
//! owned queries only, see the plan's frame rule.

use std::rc::Rc;

use bwapi::Game;

use crate::{log::Logger, reactor::sleep};

/// Frames between heartbeat lines.
const HEARTBEAT_FRAMES: i32 = 600;

/// Logs a line now and then: proof that the reactor ticks.
pub async fn heartbeat<'game>(game: Game<'game>, logger: Rc<Logger>) {
    loop {
        logger.line(game.frame_count(), "reactor: heartbeat");
        sleep(game, HEARTBEAT_FRAMES).await;
    }
}
