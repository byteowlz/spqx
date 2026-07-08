//! Zero-dependency XDG-style path resolution (byteowlz house pattern; no
//! `dirs` crate). Unix: `XDG_*_HOME` env overrides, `~/.<rel>` fallbacks.
//! Windows: `APPDATA`/`LOCALAPPDATA`.

use std::env;
use std::path::PathBuf;

use anyhow::{anyhow, Result};

pub const APP_NAME: &str = "spqx";

fn resolve_base(
    xdg: Option<PathBuf>,
    home: Option<PathBuf>,
    win_dir: Option<PathBuf>,
    is_windows: bool,
    unix_rel: &str,
) -> Option<PathBuf> {
    if let Some(path) = xdg.filter(|p| p.is_absolute()) {
        return Some(path);
    }
    if is_windows {
        win_dir
    } else {
        home.map(|home| home.join(unix_rel))
    }
}

fn base_dir(xdg_var: &str, unix_rel: &str, win_var: &str) -> Result<PathBuf> {
    resolve_base(
        env::var_os(xdg_var).map(PathBuf::from),
        env::var_os("HOME").map(PathBuf::from),
        env::var_os(win_var).map(PathBuf::from),
        cfg!(windows),
        unix_rel,
    )
    .ok_or_else(|| anyhow!("unable to determine base directory ({xdg_var})"))
}

pub fn config_dir() -> Result<PathBuf> {
    Ok(base_dir("XDG_CONFIG_HOME", ".config", "APPDATA")?.join(APP_NAME))
}

pub fn data_dir() -> Result<PathBuf> {
    Ok(base_dir("XDG_DATA_HOME", ".local/share", "APPDATA")?.join(APP_NAME))
}

pub fn state_dir() -> Result<PathBuf> {
    Ok(base_dir("XDG_STATE_HOME", ".local/state", "LOCALAPPDATA")?.join(APP_NAME))
}

pub fn cache_dir() -> Result<PathBuf> {
    Ok(base_dir("XDG_CACHE_HOME", ".cache", "LOCALAPPDATA")?.join(APP_NAME))
}

pub fn config_file() -> Result<PathBuf> {
    Ok(config_dir()?.join("config.toml"))
}

pub fn schema_file() -> Result<PathBuf> {
    Ok(config_dir()?.join("config.schema.json"))
}

pub fn voices_dir() -> Result<PathBuf> {
    Ok(data_dir()?.join("voices"))
}
