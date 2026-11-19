//! Deploys the built bot DLL into a StarCraft installation:
//! copies the DLL, wires `bwapi.ini`, and ensures the maps are present.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

const DEFAULT_SCBW: &str = r"D:\scai\scbw";
const DEFAULT_INPLACE: bool = true;

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

    // relative to the game root: BWAPI resolves `ai=` against the StarCraft.exe directory.
    let ai_value = format!("bwapi-data/ai/{BOT_DLL}");
    let ini = scbw.join("bwapi-data").join("bwapi.ini");
    let backup = env::args().fold(!DEFAULT_INPLACE, |backup, arg| match arg.as_str() {
        "--inplace" => false,
        "--backup" => true,
        _ => backup,
    });
    set_ai_keys(&ini, &ai_value, backup)?;

    sync_maps(&scbw)?;

    println!();
    println!("Ready. Start ChaosLauncher, enable BWAPI 4.4.0 Injector [RELEASE], press Start.");

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

/// Wires the bot DLL into `bwapi.ini` through the `[ai]` `ai=`/`ai_dbg=` keys.
/// Saves the original as `bwapi.ini.bak` on the first edit, unless `backup` is false.
fn set_ai_keys(ini: &Path, value: &str, backup: bool) -> Result<(), String> {
    let text = fs::read_to_string(ini).map_err(|e| format!("cannot read {}: {e}", ini.display()))?;
    let mut out = String::with_capacity(text.len());
    let mut in_ai = false;
    let mut seen = 0;
    for line in text.split_inclusive('\n') {
        let trimmed = line.trim_start();
        let is_header = trimmed.starts_with('[');
        if is_header {
            in_ai = trimmed.starts_with("[ai]");
        }
        let key = if in_ai && !is_header {
            let k = trimmed.split('=').next().unwrap_or("");
            if k == "ai" || k == "ai_dbg" { Some(k) } else { None }
        } else {
            None
        };
        match key {
            Some(k) => {
                let eol = &line[line.trim_end_matches(['\r', '\n']).len()..];
                out.push_str(k);
                out.push('=');
                out.push_str(value);
                out.push_str(eol);
                seen += 1;
            }
            None => out.push_str(line),
        }
    }
    if seen == 0 {
        return Err(format!("no ai= or ai_dbg= keys in the [ai] section of {}", ini.display()));
    }
    let bak = ini.with_extension("ini.bak");
    if backup && !bak.exists() {
        fs::copy(ini, &bak).map_err(|e| format!("cannot back up {} to {}: {e}", ini.display(), bak.display()))?;
        println!("backup: {} -> {}", ini.display(), bak.display());
    }
    fs::write(ini, &out).map_err(|e| format!("cannot write {}: {e}", ini.display()))?;
    println!("ini: ai= / ai_dbg= -> {value} ({seen} keys, {})", ini.display());
    Ok(())
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

/// Copies the bundled `maps` tree into the game without replacing existing files.
fn sync_maps(scbw: &Path) -> Result<(), String> {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("maps");
    let dst = scbw.join("maps");
    if !src.is_dir() {
        return Err(format!("bundled maps not found: {}", src.display()));
    }
    let (copied, skipped) = copy_tree(&src, &dst)?;
    println!("maps: {copied} copied, {skipped} already present");
    Ok(())
}

fn copy_tree(src: &Path, dst: &Path) -> Result<(usize, usize), String> {
    fn io_context<T>(result: std::io::Result<T>, path: &Path) -> Result<T, String> {
        result.map_err(|e| format!("{}: {e}", path.display()))
    }
    io_context(fs::create_dir_all(dst), dst)?;
    let mut copied = 0;
    let mut skipped = 0;
    for entry in io_context(fs::read_dir(src), src)? {
        let entry = io_context(entry, src)?;
        let source = entry.path();
        let target = dst.join(entry.file_name());
        let kind = io_context(entry.file_type(), &source)?;
        if kind.is_dir() {
            let (c, s) = copy_tree(&source, &target)?;
            copied += c;
            skipped += s;
        } else if kind.is_file() {
            if target.exists() {
                skipped += 1;
            } else {
                io_context(fs::copy(&source, &target), &source)?;
                println!("map: {} -> {}", source.display(), target.display());
                copied += 1;
            }
        }
    }
    Ok((copied, skipped))
}
