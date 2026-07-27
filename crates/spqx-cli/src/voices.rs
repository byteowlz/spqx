//! `spqx voices` — the named voice registry.
//!
//! Each registry voice lives at `<voices_dir>/<name>/` with `reference.wav`
//! (mono 24 kHz), `reference.txt` (transcript), and `voice.toml` (metadata).
//! `say`/`read` resolve `--voice <name>` against this directory.

use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use clap::{Args, Subcommand};
use serde::{Deserialize, Serialize};
use spqx_core::audio::{load_audio_file, resample, write_wav_file};

use crate::config::Config;
use crate::{engine, CommonOpts};

/// CustomVoice preset speakers (no reference audio required; need the
/// CustomVoice model, which is not yet wired — listed for discoverability).
const PRESET_SPEAKERS: &[&str] = &[
    "serena", "vivian", "uncle_fu", "ryan", "aiden", "ono_anna", "sohee", "eric", "dylan",
];

const STORE_SAMPLE_RATE: u32 = 24_000;

#[derive(Debug, Args)]
pub struct VoicesArgs {
    #[command(subcommand)]
    command: VoicesCommand,
}

#[derive(Debug, Subcommand)]
enum VoicesCommand {
    /// List registry voices and built-in preset speakers
    List,
    /// Add a cloned voice from a reference recording
    Add(AddArgs),
    /// Show a voice's metadata and paths
    Show { name: String },
    /// Remove a registry voice
    Rm { name: String },
}

#[derive(Debug, Args)]
struct AddArgs {
    /// Voice name (used as `--voice <name>`)
    name: String,
    /// Reference audio recording (WAV, MP3, FLAC, OGG/Vorbis, M4A/AAC)
    #[arg(long, value_name = "AUDIO")]
    ref_audio: PathBuf,
    /// Reference transcript
    #[arg(long, value_name = "TEXT")]
    ref_text: Option<String>,
    /// File containing the reference transcript
    #[arg(long, value_name = "PATH")]
    ref_text_file: Option<PathBuf>,
    /// Language hint stored with the voice (default: auto)
    #[arg(long)]
    language: Option<String>,
    /// Free-form note
    #[arg(long)]
    note: Option<String>,
    /// Overwrite an existing voice of the same name
    #[arg(long)]
    force: bool,
}

/// Per-voice metadata (`voice.toml`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct VoiceManifest {
    pub display_name: String,
    pub language: String,
    pub note: String,
    pub source: String,
    pub reference_seconds: f64,
}

impl Default for VoiceManifest {
    fn default() -> Self {
        Self {
            display_name: String::new(),
            language: "auto".to_string(),
            note: String::new(),
            source: String::new(),
            reference_seconds: 0.0,
        }
    }
}

pub fn run(args: VoicesArgs, common: &CommonOpts) -> Result<()> {
    let config = Config::load(common.config.as_deref())?;
    let dir = engine::voices_dir(&config)?;
    match args.command {
        VoicesCommand::List => list(&dir, common),
        VoicesCommand::Add(add) => add_voice(&dir, add, common),
        VoicesCommand::Show { name } => show(&dir, &name, common),
        VoicesCommand::Rm { name } => remove(&dir, &name, common),
    }
}

fn list(dir: &Path, common: &CommonOpts) -> Result<()> {
    let mut voices: Vec<(String, VoiceManifest)> = Vec::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.filter_map(|e| e.ok()) {
            let path = entry.path();
            if path.join("reference.wav").exists() && path.join("reference.txt").exists() {
                if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                    voices.push((name.to_string(), read_manifest(&path)));
                }
            }
        }
    }
    voices.sort_by(|a, b| a.0.cmp(&b.0));

    if common.json {
        let registry: Vec<_> = voices
            .iter()
            .map(|(name, m)| {
                serde_json::json!({
                    "name": name,
                    "language": m.language,
                    "reference_seconds": m.reference_seconds,
                    "note": m.note,
                })
            })
            .collect();
        let obj = serde_json::json!({
            "registry": registry,
            "presets": PRESET_SPEAKERS,
        });
        println!("{}", serde_json::to_string_pretty(&obj)?);
        return Ok(());
    }

    if voices.is_empty() {
        println!("No registry voices yet. Add one:");
        println!("  spqx voices add <name> --ref-audio <wav> --ref-text-file <txt>");
    } else {
        println!("Registry voices ({}):", voices.len());
        for (name, m) in &voices {
            let lang = if m.language == "auto" {
                String::new()
            } else {
                format!("  [{}]", m.language)
            };
            let secs = if m.reference_seconds > 0.0 {
                format!("  {:.1}s", m.reference_seconds)
            } else {
                String::new()
            };
            println!("  {name}{lang}{secs}");
        }
    }
    println!();
    println!(
        "Preset speakers (need the CustomVoice model): {}",
        PRESET_SPEAKERS.join(", ")
    );
    Ok(())
}

fn add_voice(dir: &Path, add: AddArgs, common: &CommonOpts) -> Result<()> {
    let voice_dir = dir.join(&add.name);
    if voice_dir.exists() && !add.force {
        bail!(
            "voice '{}' already exists ({}); pass --force to overwrite",
            add.name,
            voice_dir.display()
        );
    }

    let text = match (add.ref_text.as_deref(), add.ref_text_file.as_deref()) {
        (Some(text), _) => text.to_string(),
        (None, Some(file)) => std::fs::read_to_string(file)
            .with_context(|| format!("reading {}", file.display()))?
            .trim()
            .to_string(),
        (None, None) => bail!("provide --ref-text or --ref-text-file"),
    };
    if text.is_empty() {
        bail!("reference transcript is empty");
    }

    let src = add.ref_audio.to_str().context("ref path not UTF-8")?;
    let (samples, sr) = load_audio_file(src)
        .with_context(|| format!("reading reference audio {}", add.ref_audio.display()))?;
    let seconds = samples.len() as f64 / sr as f64;

    // Cheap reference hygiene — warnings, not hard errors.
    for warning in hygiene_warnings(&samples, sr, seconds) {
        if !common.quiet {
            eprintln!("warning: {warning}");
        }
    }

    let stored = if sr == STORE_SAMPLE_RATE {
        samples
    } else {
        resample(&samples, sr, STORE_SAMPLE_RATE)?
    };

    std::fs::create_dir_all(&voice_dir)
        .with_context(|| format!("creating {}", voice_dir.display()))?;
    write_wav_file(
        voice_dir.join("reference.wav").to_str().unwrap(),
        &stored,
        STORE_SAMPLE_RATE,
    )?;
    std::fs::write(voice_dir.join("reference.txt"), format!("{text}\n"))?;
    let manifest = VoiceManifest {
        display_name: add.name.clone(),
        language: add.language.unwrap_or_else(|| "auto".to_string()),
        note: add.note.unwrap_or_default(),
        source: add.ref_audio.display().to_string(),
        reference_seconds: (seconds * 10.0).round() / 10.0,
    };
    std::fs::write(
        voice_dir.join("voice.toml"),
        toml::to_string_pretty(&manifest)?,
    )?;

    if common.json {
        println!(
            "{}",
            serde_json::to_string(&serde_json::json!({
                "added": add.name,
                "path": voice_dir.display().to_string(),
                "reference_seconds": manifest.reference_seconds,
            }))?
        );
    } else {
        println!("Added voice '{}' ({})", add.name, voice_dir.display());
        println!("Try it: spqx say \"Hello.\" --voice {}", add.name);
    }
    Ok(())
}

fn show(dir: &Path, name: &str, common: &CommonOpts) -> Result<()> {
    let voice_dir = dir.join(name);
    if !voice_dir.join("reference.wav").exists() {
        bail!("voice '{name}' not found ({})", voice_dir.display());
    }
    let manifest = read_manifest(&voice_dir);
    if common.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "name": name,
                "path": voice_dir.display().to_string(),
                "language": manifest.language,
                "reference_seconds": manifest.reference_seconds,
                "note": manifest.note,
                "source": manifest.source,
            }))?
        );
    } else {
        println!("voice: {name}");
        println!("path: {}", voice_dir.display());
        println!("language: {}", manifest.language);
        println!("reference: {:.1}s", manifest.reference_seconds);
        if !manifest.note.is_empty() {
            println!("note: {}", manifest.note);
        }
        if !manifest.source.is_empty() {
            println!("source: {}", manifest.source);
        }
    }
    Ok(())
}

fn remove(dir: &Path, name: &str, common: &CommonOpts) -> Result<()> {
    let voice_dir = dir.join(name);
    if !voice_dir.exists() {
        bail!("voice '{name}' not found ({})", voice_dir.display());
    }
    if !common.yes && !common.json {
        eprint!("Remove voice '{name}' at {}? [y/N] ", voice_dir.display());
        use std::io::Write;
        std::io::stderr().flush().ok();
        let mut answer = String::new();
        std::io::stdin().read_line(&mut answer)?;
        if !matches!(answer.trim(), "y" | "Y" | "yes") {
            println!("cancelled");
            return Ok(());
        }
    }
    std::fs::remove_dir_all(&voice_dir)
        .with_context(|| format!("removing {}", voice_dir.display()))?;
    if !common.quiet {
        println!("removed voice '{name}'");
    }
    Ok(())
}

fn read_manifest(voice_dir: &Path) -> VoiceManifest {
    std::fs::read_to_string(voice_dir.join("voice.toml"))
        .ok()
        .and_then(|s| toml::from_str(&s).ok())
        .unwrap_or_default()
}

/// Cheap reference-quality checks (guideline source: fxl-6ybb / README).
fn hygiene_warnings(samples: &[f32], sr: u32, seconds: f64) -> Vec<String> {
    let mut warnings = Vec::new();
    if seconds < 6.0 {
        warnings.push(format!(
            "reference is {seconds:.1}s; 6-12s clones more reliably (short refs weaken cloning)"
        ));
    } else if seconds > 12.0 {
        warnings.push(format!(
            "reference is {seconds:.1}s; >12s can degrade first-word reliability — consider trimming to a sentence boundary"
        ));
    }
    // Isolated spike over near-silence anywhere = likely click artifact.
    let block = (sr as usize / 100).max(1); // 10ms
    let mut prev_quiet = true;
    for chunk in samples.chunks(block) {
        let peak = chunk.iter().fold(0f32, |a, s| a.max(s.abs()));
        let median = {
            let mut v: Vec<f32> = chunk.iter().map(|s| s.abs()).collect();
            v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
            v.get(v.len() / 2).copied().unwrap_or(0.0)
        };
        if peak > 0.5 && median < 0.02 && prev_quiet {
            warnings.push(
                "possible click artifact (loud spike over near-silence) — scan the reference"
                    .to_string(),
            );
            break;
        }
        prev_quiet = peak < 0.05;
    }
    warnings
}
