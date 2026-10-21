//! Pinned, file-scoped model acquisition. Never fetch a whole multi-variant repo.
#[cfg(test)]
#[path = "models_tests.rs"]
mod tests;
use crate::{config::Config, engine::expand};
use anyhow::{bail, Context, Result};
use fs2::FileExt;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Component, Path, PathBuf},
    time::{Duration, Instant},
};

#[derive(Debug, Deserialize)]
struct Asset {
    path: String,
    bytes: u64,
    sha256: String,
}
#[derive(Debug, Deserialize)]
struct Profile {
    repo: String,
    revision: String,
    files: Vec<Asset>,
}
fn profiles() -> Result<Vec<Profile>> {
    Ok(serde_json::from_str(include_str!(
        "../assets/model-profiles.json"
    ))?)
}

pub fn cache_root(config: &Config) -> Result<PathBuf> {
    if let Some(path) = std::env::var_os("SPQX_MODELS_DIR") {
        return Ok(path.into());
    }
    if let Some(path) = std::env::var_os("HF_HUB_CACHE") {
        return Ok(path.into());
    }
    if let Some(path) = std::env::var_os("HF_HOME") {
        return Ok(PathBuf::from(path).join("hub"));
    }
    if let Some(path) = &config.paths.models_dir {
        return Ok(expand(path));
    }
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .context("home directory unavailable")?;
    Ok(PathBuf::from(home).join(".cache/huggingface/hub"))
}

pub fn resolve(config: &Config) -> Result<PathBuf> {
    if let Some(path) = &config.model.path {
        let path = expand(path);
        if !path.exists() {
            bail!(
                "model.path {} does not exist (explicit paths are never downloaded)",
                path.display()
            );
        }
        return Ok(path);
    }
    let profiles = profiles()?;
    let mut id = config.model.id.as_str();
    // An explicit engine selects its compatible built-in profile. Custom IDs
    // remain cache-only, rather than silently fetching a different checkpoint.
    if profiles.iter().any(|p| p.repo == id) {
        match config.engine.backend {
            spqx_core::synthesis::Backend::Ggml => id = &profiles[1].repo,
            spqx_core::synthesis::Backend::Mlx => id = &profiles[0].repo,
            _ => (),
        }
    }
    let cache = cache_root(config)?;
    if let Some(profile) = profiles.iter().find(|p| p.repo == id) {
        if profile.repo.starts_with("mlx-community/")
            && !cfg!(all(
                target_os = "macos",
                target_arch = "aarch64",
                feature = "mlx"
            ))
        {
            bail!("MLX model requires an Apple Silicon MLX build; select --backend ggml");
        }
        let offline = config.model.offline || env_offline();
        let agent = ureq::AgentBuilder::new()
            .https_only(true)
            .timeout_connect(Duration::from_secs(20))
            .timeout_read(Duration::from_secs(90))
            .build();
        return acquire(
            profile,
            &cache,
            offline || !config.model.auto_download,
            &agent,
            "https://huggingface.co",
        );
    }
    custom_snapshot(&cache, id).with_context(|| format!("no cached model {id}; automatic downloads support only the pinned built-in profiles; set model.path for custom models"))
}

fn env_offline() -> bool {
    std::env::var("HF_HUB_OFFLINE")
        .is_ok_and(|s| matches!(s.to_lowercase().as_str(), "1" | "true" | "yes"))
}

fn custom_snapshot(cache: &Path, id: &str) -> Option<PathBuf> {
    let snapshots = cache
        .join(format!("models--{}", id.replace('/', "--")))
        .join("snapshots");
    let mut entries: Vec<_> = fs::read_dir(snapshots)
        .ok()?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    entries.sort();
    entries.pop()
}

fn valid(path: &Path, asset: &Asset) -> Result<bool> {
    let mut file = match File::open(path) {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(e) => return Err(e.into()),
    };
    if file.metadata()?.len() != asset.bytes {
        return Ok(false);
    }
    let mut hash = Sha256::new();
    let mut buffer = [0u8; 65536];
    loop {
        let n = file.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        hash.update(&buffer[..n]);
    }
    Ok(format!("{:x}", hash.finalize()) == asset.sha256)
}

fn asset_path(snapshot: &Path, asset: &Asset) -> Result<PathBuf> {
    let relative = Path::new(&asset.path);
    if asset.path.is_empty()
        || asset.path.contains('\\')
        || !relative
            .components()
            .all(|c| matches!(c, Component::Normal(_)))
    {
        bail!("unsafe model asset path");
    }
    Ok(snapshot.join(relative))
}

fn complete(profile: &Profile, snapshot: &Path) -> Result<bool> {
    for asset in &profile.files {
        if !valid(&asset_path(snapshot, asset)?, asset)? {
            return Ok(false);
        }
    }
    Ok(true)
}

fn acquire(
    profile: &Profile,
    cache: &Path,
    offline: bool,
    agent: &ureq::Agent,
    origin: &str,
) -> Result<PathBuf> {
    let repo = cache.join(format!("models--{}", profile.repo.replace('/', "--")));
    let snapshot = repo.join("snapshots").join(&profile.revision);
    if complete(profile, &snapshot)? {
        return Ok(snapshot);
    }
    if offline {
        bail!("model {}@{} is missing or corrupt in {}; offline/auto_download=false prevents downloads",profile.repo,profile.revision,cache.display());
    }
    fs::create_dir_all(&repo)?;
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(repo.join(".spqx-download.lock"))?;
    lock.lock_exclusive().context("locking model download")?;
    // Another process may have completed the snapshot while we waited.
    for asset in &profile.files {
        let path = asset_path(&snapshot, asset)?;
        if !valid(&path, asset)? {
            fetch(agent, origin, profile, asset, &path)?;
        }
    }
    Ok(snapshot)
}

fn fetch(
    agent: &ureq::Agent,
    origin: &str,
    profile: &Profile,
    asset: &Asset,
    destination: &Path,
) -> Result<()> {
    eprintln!(
        "spqx: downloading {}/{} @ {} ({} bytes)",
        profile.repo, asset.path, profile.revision, asset.bytes
    );
    let url = format!(
        "{}/{}/resolve/{}/{}",
        origin, profile.repo, profile.revision, asset.path
    );
    let response = agent
        .get(&url)
        .set("Accept-Encoding", "identity")
        .call()
        .with_context(|| format!("fetching {}/{}", profile.repo, asset.path))?;
    if let Some(length) = response.header("Content-Length") {
        if length.parse::<u64>()? != asset.bytes {
            bail!("unexpected model file length: {}", asset.path);
        }
    }
    let parent = destination.parent().context("asset parent missing")?;
    fs::create_dir_all(parent)?;
    let mut pending = tempfile::NamedTempFile::new_in(parent)?;
    let mut input = response.into_reader();
    let mut buffer = [0u8; 65536];
    let mut bytes = 0u64;
    let mut hash = Sha256::new();
    let started = Instant::now();
    loop {
        if started.elapsed() > Duration::from_secs(1800) {
            bail!("model download deadline exceeded");
        }
        let n = input.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        bytes += n as u64;
        if bytes > asset.bytes {
            bail!("oversized model file: {}", asset.path);
        }
        pending.write_all(&buffer[..n])?;
        hash.update(&buffer[..n]);
    }
    if bytes != asset.bytes || format!("{:x}", hash.finalize()) != asset.sha256 {
        bail!("model integrity check failed: {}", asset.path);
    }
    pending.as_file().sync_all()?;
    pending
        .persist(destination)
        .with_context(|| format!("committing verified model {}", destination.display()))?;
    Ok(())
}
