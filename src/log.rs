//! Match log: one line per decision point, truncated at match start.

use std::{cell::RefCell, fs::File, io::Write};

/// Append-only text log beside the game executable's working directory.
/// Decision points only (a build issued or failed, a race scouted), so the
/// file stays small even in long matches.
pub struct Logger {
    file: RefCell<Option<File>>,
}

impl Logger {
    /// Creates the file (in the BWAPI data dir when present, otherwise beside
    /// the game executable), discarding the previous match's log. Errors stay
    /// silent: logging must never break the bot.
    pub fn open() -> Logger {
        let file = File::create("bwapi-data/zzzbot.log")
            .or_else(|_| File::create("zzzbot.log"))
            .ok();
        Logger { file: RefCell::new(file) }
    }

    /// Writes one stamped line and flushes at once, so a crash keeps the
    /// log readable up to the last decision.
    pub fn line(&self, frame: i32, message: &str) {
        if let Some(file) = self.file.borrow_mut().as_mut() {
            let _ = writeln!(file, "[{frame}] {message}");
            let _ = file.flush();
        }
    }
}
