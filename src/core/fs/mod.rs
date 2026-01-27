#![allow(dead_code)]

use anyhow::Result;
use std::path::Path;

pub async fn ensure_launcher_dirs() -> Result<()> {
    let dirs = [
        crate::config::defaults::launcher_dir(),
        crate::config::defaults::profiles_dir(),
        crate::config::defaults::libraries_dir(),
        crate::config::defaults::assets_dir(),
        crate::config::defaults::versions_dir(),
        crate::config::defaults::mods_cache_dir(),
    ];

    for dir in &dirs {
        tokio::fs::create_dir_all(dir).await?;
    }

    Ok(())
}

pub async fn get_directory_size(path: &Path) -> Result<u64> {
    let mut total_size = 0;
    let mut entries = tokio::fs::read_dir(path).await?;

    while let Some(entry) = entries.next_entry().await? {
        let metadata = entry.metadata().await?;
        if metadata.is_file() {
            total_size += metadata.len();
        } else if metadata.is_dir() {
            total_size += Box::pin(get_directory_size(&entry.path())).await?;
        }
    }

    Ok(total_size)
}

pub async fn cleanup_cache() -> Result<()> {
    let cache_dir = crate::config::defaults::mods_cache_dir();
    if cache_dir.exists() {
        tokio::fs::remove_dir_all(&cache_dir).await?;
        tokio::fs::create_dir_all(&cache_dir).await?;
    }
    Ok(())
}
