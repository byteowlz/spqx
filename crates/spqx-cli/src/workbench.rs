//! Semantic voice-instrument operations used by CLI callers and the experimental editor.
use crate::{bundles::VoiceBundle, engine, CommonOpts};
use anyhow::{ensure, Context, Result};
use clap::{Args, Subcommand};
use serde::de::DeserializeOwned;
use spqx_core::{
    audio::write_wav_float_bytes,
    conditioning::{hash_bytes, FrozenConditioning, PcaSpace},
    synthesis::{Sampling, Session, Stop},
};
use std::{
    collections::BTreeMap,
    fs,
    io::Write,
    path::{Path, PathBuf},
    time::Instant,
};

#[derive(Debug, Args)]
pub struct LabArgs {
    #[command(subcommand)]
    command: LabCommand,
}
#[derive(Debug, Subcommand)]
enum LabCommand {
    /// Machine-readable JSON Schema for frozen voice bundles and their PCA banks.
    Schema,
    /// Extract a raw, engine-scoped Base speaker vector. No synthesis or enrollment.
    Extract {
        #[arg(long)]
        ref_audio: PathBuf,
        #[arg(long)]
        ref_text_file: PathBuf,
        #[arg(long, default_value = "german")]
        language: String,
        #[arg(long)]
        out: PathBuf,
    },
    /// Fit genuine PCA to independent vectors from exactly one conditioning space.
    Fit {
        /// Repeat NAME=vector.json; names are labels, not identity links.
        #[arg(long, required = true)]
        source: Vec<String>,
        #[arg(long, default_value_t = 2)]
        components: usize,
        #[arg(long, default_value_t = 0.75)]
        max_offset: f64,
        #[arg(long)]
        out: PathBuf,
    },
    /// Render an explicit candidate and freeze exactly its vector, WAV, text and bank.
    Preview {
        text: String,
        #[arg(long, conflicts_with = "space", required_unless_present = "space")]
        conditioning: Option<PathBuf>,
        #[arg(long, conflicts_with = "conditioning")]
        space: Option<PathBuf>,
        #[arg(long, requires = "space")]
        base: Option<String>,
        /// Comma-separated sigma offsets; no inference occurs until Preview is called.
        #[arg(long, requires = "space", allow_hyphen_values = true)]
        offsets: Option<String>,
        #[arg(long, default_value = "german")]
        language: String,
        #[arg(long)]
        seed: Option<u64>,
        #[arg(long)]
        out: PathBuf,
    },
    /// Explicitly promote a completed audition to the shared synthesis-only registry.
    Save {
        #[arg(long)]
        draft: PathBuf,
        #[arg(long)]
        name: String,
        /// Attach a new performance to this explicit id; never merge by display name.
        #[arg(long)]
        voice_id: Option<String>,
        /// Durable retry key supplied by the editor; never used to link by name or similarity.
        #[arg(long)]
        operation_id: Option<String>,
        #[arg(long)]
        registry: Option<PathBuf>,
    },
    /// Inspect a fitted bank and standardized coordinates without loading any model.
    Space { path: PathBuf },
    /// Inspect a saved voice, validating vector, bank, WAV and text custody.
    Inspect { path: PathBuf },
    /// List canonical spqx synthesis identities and their selectable performances.
    List {
        #[arg(long)]
        registry: Option<PathBuf>,
    },
}

pub fn run(args: LabArgs, common: &CommonOpts) -> Result<()> {
    match args.command {
        LabCommand::Schema => println!(
            "{}",
            serde_json::to_string_pretty(&schemars::schema_for!(VoiceBundle))?
        ),
        LabCommand::Extract {
            ref_audio,
            ref_text_file,
            language,
            out,
        } => {
            let config = common.load_config()?;
            let text = fs::read_to_string(ref_text_file)?;
            let model = engine::resolve_model_dir(&config)?;
            let _guard = spqx_core::stdio::StdoutGuard::new()?;
            let session = Session::load(
                &model,
                &ref_audio,
                &text,
                &spqx_core::worker::normalize_language(&language),
                &config.engine,
            )?;
            let vector = session.frozen_conditioning()?;
            write_json(&out, &vector)?;
            drop(_guard);
            println!(
                "{}",
                serde_json::json!({"conditioning":out,"scope":vector.scope,"recognition_enrolled":false})
            );
        }
        LabCommand::Fit {
            source,
            components,
            max_offset,
            out,
        } => {
            let mut vectors = BTreeMap::new();
            let mut scope = None;
            for entry in source {
                let (name, file) = entry
                    .split_once('=')
                    .context("source must be NAME=vector.json")?;
                let frozen: FrozenConditioning = read_json(Path::new(file))?;
                frozen.validate()?;
                if let Some(scope) = &scope {
                    frozen.require_scope(scope)?;
                } else {
                    scope = Some(frozen.scope.clone());
                }
                ensure!(
                    vectors.insert(name.to_owned(), frozen.values).is_none(),
                    "duplicate PCA source name"
                );
            }
            let space = PcaSpace::fit(
                scope.context("no source vectors")?,
                vectors,
                components,
                max_offset,
            )?;
            write_json(&out, &space)?;
            print_space(&space)?;
        }
        LabCommand::Preview {
            text,
            conditioning,
            space,
            base,
            offsets,
            language,
            seed,
            out,
        } => {
            ensure!(
                !out.exists(),
                "draft already exists; it will not be overwritten"
            );
            let pca: Option<PcaSpace> = space.as_deref().map(read_json).transpose()?;
            let frozen = if let Some(pca) = &pca {
                let base = base.as_deref().context("PCA Preview requires --base")?;
                let offsets: Vec<f64> = offsets
                    .context("PCA Preview requires --offsets")?
                    .split(',')
                    .map(str::parse)
                    .collect::<std::result::Result<_, _>>()?;
                pca.edit(base, &offsets)?
            } else {
                read_json(conditioning.as_deref().context("missing conditioning")?)?
            };
            let config = common.load_config()?;
            let model = engine::resolve_model_dir(&config)?;
            let language = spqx_core::worker::normalize_language(&language);
            let start = Instant::now();
            let _guard = spqx_core::stdio::StdoutGuard::new()?;
            let session = Session::load_frozen(&model, &frozen, &language, seed, &config.engine)?;
            let prepare_s = start.elapsed().as_secs_f64();
            let (wave, first_pcm_s, generation_s) = render(&session, &text, &config)?;
            drop(_guard);
            let voice = VoiceBundle {
                version: 1,
                conditioning: frozen,
                pca,
                text,
                language,
                seed,
                reference_sha256: hash_bytes(&wave),
                provenance: [
                    (
                        "creator".into(),
                        "spqx native voice instrument; no training or enrollment".into(),
                    ),
                    (
                        "temperature".into(),
                        config.sampling.temperature.to_string(),
                    ),
                    ("top_k".into(), config.sampling.top_k.to_string()),
                ]
                .into(),
            };
            voice.save(&out, &wave)?;
            println!(
                "{}",
                serde_json::json!({"draft":out,"scope":voice.conditioning.scope,"recipe":voice.conditioning.recipe,"reference_sha256":voice.reference_sha256,"prepare_s":prepare_s,"first_pcm_s_after_prepare":first_pcm_s,"generation_s":generation_s,"total_s":start.elapsed().as_secs_f64(),"canonical_voice_id":null,"registry_promoted":false})
            );
        }
        LabCommand::Save {
            draft,
            name,
            voice_id,
            operation_id,
            registry,
        } => {
            let id = crate::bundles::promote_once(
                &draft,
                &name,
                voice_id.as_deref(),
                registry.as_deref(),
                operation_id.as_deref(),
            )?;
            println!(
                "{}",
                serde_json::json!({"voice_id":id,"registry_promoted":true,"recognition_enrolled":false})
            );
        }
        LabCommand::Space { path } => print_space(&read_json::<PcaSpace>(&path)?)?,
        LabCommand::Inspect { path } => {
            let (bundle, _) = VoiceBundle::load(&path)?;
            println!("{}", serde_json::to_string_pretty(&bundle)?);
        }
        LabCommand::List { registry } => {
            let data = crate::bundles::store(registry.as_deref())?.load()?;
            let voices:Vec<_>=data.identities.iter().filter_map(|v|v.synthesis_profile(crate::bundles::ENGINE).map(|p|serde_json::json!({"voice_id":v.id,"display_name":v.display_name,"references":p.references}))).collect();
            println!("{}", serde_json::to_string_pretty(&voices)?);
        }
    }
    Ok(())
}
fn render(
    session: &Session,
    text: &str,
    config: &crate::config::Config,
) -> Result<(Vec<u8>, Option<f64>, f64)> {
    let sampling = Sampling {
        temperature: config.sampling.temperature,
        top_k: config.sampling.top_k,
        max_new_tokens: 256,
        chunk_size: 4,
    };
    let start = Instant::now();
    let mut first = None;
    let mut samples = Vec::new();
    let stop = session.synthesize(
        text,
        &sampling,
        |chunk, rate| {
            assert_eq!(rate, 24000);
            first.get_or_insert_with(|| start.elapsed().as_secs_f64());
            samples.extend_from_slice(chunk);
            true
        },
        || false,
    )?;
    ensure!(
        stop == Stop::Eos,
        "incomplete/cancelled/capped candidate; no saved voice"
    );
    let seconds = start.elapsed().as_secs_f64();
    ensure!(
        samples.iter().all(|v| v.is_finite() && v.abs() <= 1.),
        "invalid native PCM"
    );
    Ok((write_wav_float_bytes(&samples, 24000)?, first, seconds))
}
fn print_space(space: &PcaSpace) -> Result<()> {
    let coordinates: BTreeMap<_, _> = space
        .sources
        .iter()
        .map(|(id, v)| Ok((id, space.coordinates(v)?)))
        .collect::<Result<_>>()?;
    println!(
        "{}",
        serde_json::json!({"space":space.id()?,"scope":space.scope,"components":space.components.len(),"max_offset":space.max_offset,"coordinates":coordinates,"semantic_axis_labels":false,"independent_sources":space.sources.len()})
    );
    Ok(())
}
fn read_json<T: DeserializeOwned>(path: &Path) -> Result<T> {
    ensure!(
        fs::metadata(path)?.len() <= 16 * 1024 * 1024,
        "conditioning/bank file too large"
    );
    Ok(serde_json::from_slice(&fs::read(path)?)?)
}
fn write_json(path: &Path, value: &impl Serialize) -> Result<()> {
    if let Some(parent) = path.parent().filter(|v| !v.as_os_str().is_empty()) {
        fs::create_dir_all(parent)?;
    }
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)?;
    file.write_all(&serde_json::to_vec_pretty(value)?)?;
    file.sync_all()?;
    Ok(())
}
use serde::Serialize;
