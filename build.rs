//! Compile-time stamp so every match log states exactly which binary ran:
//! a stale DLL (SC process not restarted) reads as an old stamp.

fn main() {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    println!("cargo:rustc-env=BUILD_UNIX_TIME={secs}");
}
