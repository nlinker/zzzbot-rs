# zzzkbot-rs

StarCraft: Brood War bot on [bwapi-rs](../bwapi-rs). Zerg logic inspired by
[ZZZKBot](https://github.com/bwapi/bwapi/wiki/BWAPI-Bots); the implementation is original.

- `src/lib.rs` — the bot, built as a cdylib (`zzzkbot.dll`) loaded by BWAPI
- `src/main.rs` — `deploy` binary: copies the DLL into a StarCraft installation,
  wires `bwapi.ini`, ensures the map is present

Target is `i686-pc-windows-msvc` (the game is a 32-bit process); pinned via
`.cargo/config.toml`, so plain `cargo build` / `cargo run` need no flags.
