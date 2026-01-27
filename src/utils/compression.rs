#![allow(dead_code)]

use anyhow::Result;
use std::path::Path;
use std::fs::File;

pub fn extract_zip(zip_path: &Path, destination: &Path) -> Result<()> {
    let file = File::open(zip_path)?;
    let mut archive = zip::ZipArchive::new(file)?;

    for i in 0..archive.len() {
        let mut file = archive.by_index(i)?;
        let outpath = destination.join(file.name());

        if file.name().ends_with('/') {
            std::fs::create_dir_all(&outpath)?;
        } else {
            if let Some(parent) = outpath.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let mut outfile = File::create(&outpath)?;
            std::io::copy(&mut file, &mut outfile)?;
        }
    }

    Ok(())
}

pub fn compress_directory(source: &Path, output: &Path) -> Result<()> {
    let file = File::create(output)?;
    let mut zip = zip::ZipWriter::new(file);

    let options = zip::write::FileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .unix_permissions(0o755);

    let walkdir = walkdir::WalkDir::new(source);
    let it = walkdir.into_iter();

    for entry in it.filter_map(|e| e.ok()) {
        let path = entry.path();
        let name = path.strip_prefix(source)?;

        if path.is_file() {
            zip.start_file(name.to_string_lossy().to_string(), options)?;
            let mut f = File::open(path)?;
            std::io::copy(&mut f, &mut zip)?;
        } else if !name.as_os_str().is_empty() {
            zip.add_directory(name.to_string_lossy().to_string(), options)?;
        }
    }

    zip.finish()?;
    Ok(())
}
