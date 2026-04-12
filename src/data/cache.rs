use anyhow::{Context, Result};
use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use flate2::Compression;
use serde::de::DeserializeOwned;
use serde::Serialize;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use crate::data::paths::data_dir;

pub fn cache_dir() -> PathBuf {
    downloads_dir_for("backtest")
}

/// Directory for unified (parquet) cache files: `data/unified/{TICKER}/`.
/// Separate from per-day source cache so they never conflict.
pub fn unified_dir() -> PathBuf {
    let dir = data_dir().join("unified");
    fs::create_dir_all(&dir).ok();
    dir
}

pub fn downloads_dir_for(scope: &str) -> PathBuf {
    let dir = data_dir().join("downloads").join(scope);
    fs::create_dir_all(&dir).ok();
    dir
}

pub fn raw_path(kind: &str, ticker: &str, date: &str) -> PathBuf {
    raw_path_for("backtest", kind, ticker, date)
}

pub fn raw_path_for(scope: &str, kind: &str, ticker: &str, date: &str) -> PathBuf {
    let dir = day_dir_for(scope, ticker, date);
    fs::create_dir_all(&dir).ok();
    dir.join(format!("raw_{kind}.json.gz"))
}

/// Per-day directory: `data/downloads/{scope}/{TICKER}/{YYYY-MM}/{YYYY-MM-DD}/`
pub fn day_dir_for(scope: &str, ticker: &str, date: &str) -> PathBuf {
    let month = &date[..7]; // YYYY-MM
    downloads_dir_for(scope).join(ticker).join(month).join(date)
}

/// Read a JSON file from disk (per-day bars, raw data, etc.).
pub fn read_processed<T: DeserializeOwned>(path: &Path) -> Option<T> {
    if !path.exists() {
        return None;
    }
    match read_json(path) {
        Ok(val) => Some(val),
        Err(e) => {
            eprintln!("[cache] Corrupt cache file {}: {}", path.display(), e);
            fs::remove_file(path).ok();
            None
        }
    }
}

pub fn read_raw_gz<T: DeserializeOwned>(path: &Path) -> Option<T> {
    if !path.exists() {
        return None;
    }
    match read_gz_json(path) {
        Ok(val) => Some(val),
        Err(e) => {
            eprintln!("[cache] Corrupt raw cache file {}: {}", path.display(), e);
            fs::remove_file(path).ok();
            None
        }
    }
}

pub fn write_raw_gz<T: Serialize>(path: &Path, data: &T) -> Result<()> {
    let json = serde_json::to_string(data)?;
    let tmp = path.with_extension("json.gz.tmp");
    let file = fs::File::create(&tmp)?;
    let mut encoder = GzEncoder::new(file, Compression::default());
    encoder.write_all(json.as_bytes())?;
    encoder.finish()?;
    fs::rename(&tmp, path).context("rename tmp -> final")?;
    Ok(())
}

pub fn write_processed<T: Serialize>(path: &Path, data: &T) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).ok();
    }
    let tmp = path.with_extension("json.tmp");
    let json = serde_json::to_string(data)?;
    fs::write(&tmp, json)?;
    fs::rename(&tmp, path).context("rename tmp -> final")?;
    Ok(())
}

fn read_json<T: DeserializeOwned>(path: &Path) -> Result<T> {
    let json = fs::read_to_string(path)?;
    let val: T = serde_json::from_str(&json)?;
    Ok(val)
}

fn read_gz_json<T: DeserializeOwned>(path: &Path) -> Result<T> {
    let file = fs::File::open(path)?;
    let mut decoder = GzDecoder::new(file);
    let mut json = String::new();
    decoder.read_to_string(&mut json)?;
    let val: T = serde_json::from_str(&json)?;
    Ok(val)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raw_path_structure() {
        let p = raw_path("theta", "GOOGL", "2024-06-01");
        let s = p.to_string_lossy();
        assert!(s.contains("GOOGL"));
        assert!(s.contains("2024-06-01"));
        assert!(s.ends_with("raw_theta.json.gz"));
    }

    #[test]
    fn raw_gz_roundtrip() {
        let dir = std::env::temp_dir().join("gex_cache_test_gz");
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("raw_test.json.gz");

        let data: Vec<u32> = vec![10, 20, 30];
        write_raw_gz(&path, &data).unwrap();
        let loaded: Vec<u32> = read_raw_gz(&path).unwrap();
        assert_eq!(data, loaded);

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn read_nonexistent_returns_none() {
        let path = std::env::temp_dir().join("gex_cache_test_nonexistent.json");
        let result: Option<Vec<u32>> = read_processed(&path);
        assert!(result.is_none());
    }
}
