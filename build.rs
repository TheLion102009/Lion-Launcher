fn main() {
    embed_build_info();
    tauri_build::build()
}

/// Bettet Commit und Release-Kanal in die Binary ein (für den Auto-Updater).
///
/// * `LION_GIT_SHA`: Commit, aus dem gebaut wurde (CI: `GITHUB_SHA`, lokal: `git rev-parse HEAD`)
/// * `LION_BUILD_CHANNEL`: `stable` (Tag `v*`), `nightly` (sonstiger CI-Build) oder `dev` (lokal)
fn embed_build_info() {
    for var in [
        "GITHUB_SHA",
        "GITHUB_ACTIONS",
        "GITHUB_REF_TYPE",
        "GITHUB_REF_NAME",
    ] {
        println!("cargo:rerun-if-env-changed={var}");
    }
    println!("cargo:rerun-if-changed=.git/HEAD");

    let sha = std::env::var("GITHUB_SHA")
        .ok()
        .filter(|s| !s.is_empty())
        .or_else(|| {
            std::process::Command::new("git")
                .args(["rev-parse", "HEAD"])
                .output()
                .ok()
                .filter(|o| o.status.success())
                .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        })
        .unwrap_or_default();

    let in_ci = std::env::var("GITHUB_ACTIONS").is_ok_and(|v| v == "true");
    let is_version_tag = std::env::var("GITHUB_REF_TYPE").is_ok_and(|v| v == "tag")
        && std::env::var("GITHUB_REF_NAME").is_ok_and(|v| v.starts_with('v'));
    let channel = match (in_ci, is_version_tag) {
        (true, true) => "stable",
        (true, false) => "nightly",
        _ => "dev",
    };

    println!("cargo:rustc-env=LION_GIT_SHA={sha}");
    println!("cargo:rustc-env=LION_BUILD_CHANNEL={channel}");
}
