//! Pinned, file-scoped model acquisition. Never fetch a whole multi-variant repo.
#[cfg(test)]
#[path = "models_tests.rs"]
mod tests;
use crate::{config::Config, engine::expand};
use anyhow::{bail, Context, Result};
use fs2::FileExt;
use indicatif::{ProgressBar, ProgressStyle};
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, clap::ValueEnum)]
#[serde(rename_all = "snake_case")]
pub enum Purpose {
    #[default]
    Clone,
    Design,
}

#[derive(Debug, Deserialize)]
struct Profile {
    purpose: Purpose,
    backend: spqx_core::synthesis::Backend,
    #[serde(default)]
    entrypoint: Option<String>,
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
    resolve_for(config, Purpose::Clone)
}

pub fn resolve_for(config: &Config, purpose: Purpose) -> Result<PathBuf> {
    // Cloning settings never select VoiceDesign weights, and vice versa.
    let (key, explicit) = match purpose {
        Purpose::Clone => ("model.path", &config.model.path),
        Purpose::Design => ("model.design_path", &config.model.design_path),
    };
    if let Some(path) = explicit {
        let path = expand(path);
        if !path.exists() {
            bail!(
                "{key} {} does not exist (explicit paths are never downloaded)",
                path.display()
            );
        }
        return Ok(path);
    }
    if config.engine.backend == spqx_core::synthesis::Backend::Tch {
        bail!("legacy tch requires explicit {key}; automatic downloads support GGML and MLX");
    }
    let profiles = profiles()?;
    let profile = select_profile(&profiles, &config.model.id, config.engine.backend, purpose)?;
    let cache = cache_root(config)?;
    if let Some(profile) = profile {
        if profile.backend == spqx_core::synthesis::Backend::Mlx
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
        let snapshot = acquire(
            profile,
            &cache,
            offline || !config.model.auto_download,
            &agent,
            "https://huggingface.co",
        )?;
        return match &profile.entrypoint {
            Some(name) => {
                let asset = profile
                    .files
                    .iter()
                    .find(|asset| &asset.path == name)
                    .context("model entrypoint is not a pinned asset")?;
                asset_path(&snapshot, asset)
            }
            None => Ok(snapshot),
        };
    }
    let id = &config.model.id;
    custom_snapshot(&cache, id).with_context(|| format!("no cached model {id}; automatic downloads support only the pinned built-in profiles; set model.path for custom models"))
}

fn select_profile<'a>(
    profiles: &'a [Profile],
    id: &str,
    backend: spqx_core::synthesis::Backend,
    purpose: Purpose,
) -> Result<Option<&'a Profile>> {
    let known = profiles.iter().find(|profile| profile.repo == id);
    if known.is_none() && purpose == Purpose::Clone {
        return Ok(None); // Custom cloning model IDs remain cache-only.
    }
    // Auto follows a built-in configured model's engine, else this build's default.
    let backend = match (backend, known) {
        (spqx_core::synthesis::Backend::Auto, Some(known)) => known.backend,
        (spqx_core::synthesis::Backend::Auto, None) => default_backend(),
        (explicit, _) => explicit,
    };
    let profile = profiles.iter().find(|profile| profile.backend == backend && profile.purpose == purpose)
        .with_context(|| format!("no compatible pinned {purpose:?} profile for {backend:?}; this operation is not implemented for that engine (no Base model or backend fallback)"))?;
    Ok(Some(profile))
}

fn default_backend() -> spqx_core::synthesis::Backend {
    if cfg!(all(
        target_os = "macos",
        target_arch = "aarch64",
        feature = "mlx"
    )) {
        spqx_core::synthesis::Backend::Mlx
    } else {
        spqx_core::synthesis::Backend::Ggml
    }
}

fn env_offline() -> bool {
    std::env::var("HF_HUB_OFFLINE")
        .is_ok_and(|s| matches!(s.to_lowercase().as_str(), "1" | "true" | "yes" | "on"))
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
        bail!("model {}@{} ({:?}/{:?}, {}) is missing or corrupt in {}; offline/auto_download=false prevents downloads", profile.repo, profile.revision, profile.backend, profile.purpose, profile.entrypoint.as_deref().unwrap_or("snapshot"), cache.display());
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
    let progress = download_progress(asset.bytes);
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
        progress.set_position(bytes);
    }
    progress.set_message("verifying");
    if bytes != asset.bytes || format!("{:x}", hash.finalize()) != asset.sha256 {
        bail!("model integrity check failed: {}", asset.path);
    }
    pending.as_file().sync_all()?;
    pending
        .persist(destination)
        .with_context(|| format!("committing verified model {}", destination.display()))?;
    progress.finish_and_clear();
    Ok(())
}

fn download_progress(total: u64) -> ProgressBar {
    let progress = ProgressBar::new(total);
    // indicatif draws on stderr only when it is an interactive terminal.
    progress.set_style(
        ProgressStyle::with_template(
            "  [{bar:32.cyan/blue}] {bytes}/{total_bytes} {bytes_per_sec} ETA {eta} {msg}",
        )
        .unwrap_or_else(|_| ProgressStyle::default_bar()),
    );
    progress
}
