//! spqx — fast local text-to-speech with voice cloning.
//!
//! The human/agent-facing CLI over the spqx engine. The persistent worker
//! binary (`spqx-tts-worker`) remains flag-only and config-free for
//! low-latency runtimes like foxline; this CLI is the interface for
//! everything else.

mod bundles;
mod config;
mod design;
mod engine;
mod paths;
mod playback;
mod say;
mod serve;
mod voices;
mod workbench;

use std::io::Write;
use std::path::PathBuf;

use anyhow::Result;
use clap::{Args, CommandFactory, Parser, Subcommand};
use clap_complete::Shell;

use config::Config;

#[derive(Debug, Parser)]
#[command(
    name = "spqx",
    version,
    about = "Fast local text-to-speech with voice cloning",
    propagate_version = true
)]
struct Cli {
    #[command(flatten)]
    common: CommonOpts,
    #[command(subcommand)]
    command: Command,
}

/// Global options shared by all subcommands.
#[derive(Debug, Clone, Args)]
pub struct CommonOpts {
    /// Whole-pipeline engine (model format controls auto selection).
    #[arg(long, value_enum, global = true, env = "SPQX_BACKEND")]
    backend: Option<spqx_core::synthesis::Backend>,
    /// Compute device: auto, cpu, cuda:0, vulkan:0, or GGML device name.
    #[arg(long, global = true, env = "SPQX_DEVICE")]
    device: Option<String>,
    /// Trusted pinned GGML runtime library path.
    #[arg(long, global = true, env = "SPQX_ENGINE_LIBRARY")]
    engine_library: Option<PathBuf>,
    /// GGML codec/tokenizer GGUF path.
    #[arg(long, global = true, env = "SPQX_CODEC")]
    codec: Option<PathBuf>,
    /// Override the config file path (also: SPQX_CONFIG)
    #[arg(long, value_name = "PATH", global = true, env = "SPQX_CONFIG")]
    config: Option<PathBuf>,
    /// Output machine-readable JSON
    #[arg(long, global = true)]
    json: bool,
    /// Reduce output to errors only
    #[arg(short, long, global = true)]
    quiet: bool,
    /// Increase logging verbosity (stackable)
    #[arg(short = 'v', long = "verbose", action = clap::ArgAction::Count, global = true)]
    verbose: u8,
    /// Enable debug logging (equivalent to -vv)
    #[arg(long, global = true)]
    debug: bool,
    /// Disable ANSI colors
    #[arg(long = "no-color", global = true)]
    no_color: bool,
    /// Assume "yes" for prompts
    #[arg(short = 'y', long = "yes", global = true)]
    yes: bool,
}

impl CommonOpts {
    fn load_config(&self) -> Result<Config> {
        let mut config = Config::load(self.config.as_deref())?;
        if let Some(backend) = self.backend {
            config.engine.backend = backend;
        }
        if let Some(device) = &self.device {
            config.engine.device = device.clone();
        }
        if let Some(path) = &self.engine_library {
            config.engine.library = Some(path.clone());
        }
        if let Some(path) = &self.codec {
            config.engine.codec = Some(path.clone());
        }
        Ok(config)
    }
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Speak text with live playback and/or WAV output
    Say(say::SayArgs),
    /// Generate a new described voice candidate using a native VoiceDesign checkpoint
    Design(design::DesignArgs),
    /// Fit/edit real PCA conditioning, preview exact frozen drafts and explicitly save voices
    Lab(workbench::LabArgs),
    /// Manage the named voice registry
    Voices(voices::VoicesArgs),
    /// Run the binary-framed worker protocol on stdin/stdout
    Serve(serve::ServeArgs),
    /// Inspect and manage configuration
    Config {
        #[command(subcommand)]
        command: ConfigCommand,
    },
    /// Generate shell completions
    Completions {
        #[arg(value_enum)]
        shell: Shell,
    },
}

#[derive(Debug, Clone, Copy, Subcommand)]
enum ConfigCommand {
    /// Print the effective configuration
    Show,
    /// Print the resolved config file path
    Path,
    /// Print all resolved directories (config, data, state, cache, voices)
    Paths,
    /// Print the JSON schema for the config file
    Schema,
    /// Write the default config and schema (keeps an existing config)
    Reset,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    init_logging(&cli.common);

    match cli.command {
        Command::Say(args) => say::run(args, &cli.common),
        Command::Design(args) => design::run(args, &cli.common),
        Command::Lab(args) => workbench::run(args, &cli.common),
        Command::Voices(args) => voices::run(args, &cli.common),
        Command::Serve(args) => serve::run(args, &cli.common),
        Command::Config { command } => run_config(command, &cli.common),
        Command::Completions { shell } => {
            let mut cmd = Cli::command();
            clap_complete::generate(shell, &mut cmd, "spqx", &mut std::io::stdout());
            Ok(())
        }
    }
}

fn init_logging(common: &CommonOpts) {
    let level = if common.quiet {
        log::LevelFilter::Error
    } else if common.debug || common.verbose >= 2 {
        log::LevelFilter::Debug
    } else if common.verbose == 1 {
        log::LevelFilter::Info
    } else {
        log::LevelFilter::Warn
    };
    let mut builder = env_logger::Builder::from_env(env_logger::Env::default());
    builder.filter_level(level);
    if common.no_color || std::env::var_os("NO_COLOR").is_some() {
        builder.write_style(env_logger::WriteStyle::Never);
    }
    let _ = builder.try_init();
}

fn run_config(command: ConfigCommand, common: &CommonOpts) -> Result<()> {
    match command {
        ConfigCommand::Show => {
            let config = common.load_config()?;
            let out = if common.json {
                serde_json::to_string_pretty(&config)?
            } else {
                toml::to_string_pretty(&config)?
            };
            println!("{out}");
        }
        ConfigCommand::Path => {
            let path = match &common.config {
                Some(path) => path.clone(),
                None => paths::config_file()?,
            };
            println!("{}", path.display());
        }
        ConfigCommand::Paths => {
            let entries = [
                ("config", paths::config_dir()?),
                ("data", paths::data_dir()?),
                ("state", paths::state_dir()?),
                ("cache", paths::cache_dir()?),
                ("voices", paths::voices_dir()?),
            ];
            if common.json {
                let map: serde_json::Map<String, serde_json::Value> = entries
                    .iter()
                    .map(|(name, path)| ((*name).to_string(), path.display().to_string().into()))
                    .collect();
                println!("{}", serde_json::to_string_pretty(&map)?);
            } else {
                for (name, path) in entries {
                    println!("{name}: {}", path.display());
                }
            }
        }
        ConfigCommand::Schema => {
            let mut stdout = std::io::stdout();
            writeln!(stdout, "{}", Config::schema_json()?)?;
        }
        ConfigCommand::Reset => {
            let path = match &common.config {
                Some(path) => path.clone(),
                None => paths::config_file()?,
            };
            let schema = Config::default().write_default(&path)?;
            println!("config: {}", path.display());
            println!("schema: {}", schema.display());
        }
    }
    Ok(())
}
