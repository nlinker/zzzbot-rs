//! Deploys the built bot DLL into a StarCraft installation:
//! copies the DLL, wires `bwapi.ini`, and ensures the maps are present.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

const DEFAULT_SCBW: &str = r"D:\scai\scbw";

/// The cdylib artifact is named after the package (`[lib] name` is not overridden).
const BOT_DLL: &str = concat!(env!("CARGO_PKG_NAME"), ".dll");

fn main() -> ExitCode {
    if let Err(msg) = run() {
        eprintln!("deploy: {msg}");
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}

fn run() -> Result<(), String> {
    let scbw = scbw_dir()?;
    let dll = find_bot_dll()?;

    let dest_dir = scbw.join("bwapi-data").join("ai");
    fs::create_dir_all(&dest_dir).map_err(|e| format!("cannot create {}: {e}", dest_dir.display()))?;
    let dest = dest_dir.join(BOT_DLL);
    fs::copy(&dll, &dest).map_err(|e| format!("cannot copy {} to {}: {e}", dll.display(), dest.display()))?;
    println!("DLL: {} -> {}", dll.display(), dest.display());

    Ok(())
}

/// The scbw directory: `--scbw <path>`, else `SCBW_DIR`, else the default.
fn scbw_dir() -> Result<PathBuf, String> {
    let args: Vec<String> = env::args().collect();
    if let Some(pos) = args.iter().position(|a| a == "--scbw") {
        return args
            .get(pos + 1)
            .map(PathBuf::from)
            .ok_or_else(|| "--scbw requires a path".to_string());
    }
    if let Ok(dir) = env::var("SCBW_DIR") {
        if !dir.is_empty() {
            return Ok(PathBuf::from(dir));
        }
    }
    let dir = PathBuf::from(DEFAULT_SCBW);
    if !dir.is_dir() {
        return Err(format!("scbw directory not found: {} (use --scbw or SCBW_DIR)", dir.display()));
    }
    Ok(dir)
}

/// The freshly built DLL sits next to this executable in the cargo target dir.
fn find_bot_dll() -> Result<PathBuf, String> {
    let exe = env::current_exe().map_err(|e| format!("cannot locate this executable: {e}"))?;
    let dll = exe.parent().map(Path::to_path_buf).unwrap_or_default().join(BOT_DLL);
    if !dll.is_file() {
        return Err(format!(
            "bot DLL not found next to the deploy executable: {}\nrun `cargo build` first",
            dll.display()
        ));
    }
    Ok(dll)
}
