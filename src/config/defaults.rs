use std::path::PathBuf;

pub fn launcher_dir() -> PathBuf {
    directories::ProjectDirs::from("com", "lionlauncher", "Lion-Launcher")
        .map(|dirs| dirs.data_dir().to_path_buf())
        .unwrap_or_else(|| PathBuf::from(".lion-launcher"))
}

pub fn data_dir() -> PathBuf {
    launcher_dir()
}

pub fn profiles_dir() -> PathBuf {
    launcher_dir().join("profiles")
}

pub fn libraries_dir() -> PathBuf {
    launcher_dir().join("libraries")
}

pub fn assets_dir() -> PathBuf {
    launcher_dir().join("assets")
}

pub fn versions_dir() -> PathBuf {
    launcher_dir().join("versions")
}

pub fn mods_cache_dir() -> PathBuf {
    launcher_dir().join("cache").join("mods")
}

pub fn shared_settings_file() -> PathBuf {
    launcher_dir().join("shared_options.txt")
}

pub fn default_memory_mb() -> u32 {
    4096
}

pub fn default_java_args() -> Vec<String> {
    vec![
        "-XX:+UnlockExperimentalVMOptions".to_string(),
        "-XX:+UseG1GC".to_string(),
        "-XX:G1NewSizePercent=20".to_string(),
        "-XX:G1ReservePercent=20".to_string(),
        "-XX:MaxGCPauseMillis=50".to_string(),
        "-XX:G1HeapRegionSize=32M".to_string(),
    ]
}
