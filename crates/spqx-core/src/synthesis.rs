//! Whole-pipeline prepared synthesis shared by CLI and framed workers.
use anyhow::{bail, Context, Result};
use clap::ValueEnum;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Complete model execution engine, distinct from its compute device.
#[derive(
    Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, ValueEnum,
)]
#[serde(rename_all = "snake_case")]
pub enum Backend {
    /// Select by model format and compiled availability; never uses a hosted service.
    #[default]
    Auto,
    /// Apple Silicon MLX.
    Mlx,
    /// Portable whole-pipeline GGML/libqwen.
    Ggml,
    /// Legacy dense libtorch engine, when compiled in.
    Tch,
}
/// Engine settings; explicit device requests do not silently fall back.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct EngineOptions {
    /// Execution engine.
    pub backend: Backend,
    /// auto, cpu, cuda[:N], vulkan[:N], or an explicit GGML device name.
    pub device: String,
    /// GGML tokenizer/codec GGUF; not needed for a native model directory.
    pub codec: Option<PathBuf>,
    /// Trusted pinned native runtime. Otherwise only executable-relative paths are searched.
    pub library: Option<PathBuf>,
}
impl Default for EngineOptions {
    fn default() -> Self {
        Self {
            backend: Backend::Auto,
            device: "auto".into(),
            codec: None,
            library: None,
        }
    }
}
/// Per-request sampling; reference state is prepared independently.
#[derive(Debug, Clone)]
pub struct Sampling {
    /// Sampling temperature.
    pub temperature: f64,
    /// Top-k sampling cutoff.
    pub top_k: i64,
    /// Maximum codec frames. Reaching it is not successful completion.
    pub max_new_tokens: i64,
    /// Native engine streaming chunk size.
    pub chunk_size: usize,
}
/// Why synthesis ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stop {
    /// End-of-sequence before the frame cap.
    Eos,
    /// Output hit its hard frame bound.
    CapReached,
    /// Caller requested cancellation.
    Cancelled,
}
impl Stop {
    /// A rejected audio callback must win even over an already observed EOS.
    pub(crate) fn after_callback(self, keep_running: bool) -> Self {
        if keep_running {
            self
        } else {
            Self::Cancelled
        }
    }
    /// Whether the utterance was truncated.
    pub fn truncated(self) -> bool {
        self == Self::CapReached
    }
    /// Stable status-event label.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Eos => "eos",
            Self::CapReached => "cap_reached",
            Self::Cancelled => "cancelled",
        }
    }
}
/// Resolve engine selection without loading weights or allocating an accelerator.
pub fn select_backend(model: &Path, requested: Backend) -> Result<Backend> {
    let gguf = model.extension().is_some_and(|e| e == "gguf")
        || model.join("qwen-talker-0.6b-base-Q8_0.gguf").is_file();
    let selected = match requested {
        Backend::Auto if gguf => Backend::Ggml,
        Backend::Auto if cfg!(spqx_mlx) => Backend::Mlx,
        Backend::Auto if cfg!(feature = "tch-backend") => Backend::Tch,
        Backend::Auto => {
            bail!("this build requires GGUF model assets; select a GGUF model and codec")
        }
        other => other,
    };
    let available = match selected {
        Backend::Mlx => cfg!(spqx_mlx),
        Backend::Tch => cfg!(feature = "tch-backend"),
        Backend::Ggml => cfg!(feature = "qwentts"),
        Backend::Auto => false,
    };
    if !available {
        bail!("requested engine {selected:?} is not available in this build/target");
    }
    Ok(selected)
}
/// Canonical GGML device name. None deliberately delegates auto selection to GGML.
pub fn ggml_device(device: &str) -> Result<Option<String>> {
    let value = device.trim();
    if value.eq_ignore_ascii_case("auto") {
        return Ok(None);
    }
    if value.eq_ignore_ascii_case("cpu") {
        return Ok(Some("CPU".into()));
    }
    for (prefix, canonical) in [("cuda", "CUDA"), ("vulkan", "Vulkan"), ("sycl", "SYCL")] {
        if value.eq_ignore_ascii_case(prefix) {
            return Ok(Some(format!("{canonical}0")));
        }
        if let Some((head, index)) = value.split_once(':') {
            if head.eq_ignore_ascii_case(prefix) {
                let index: u32 = index
                    .parse()
                    .context("device index must be a nonnegative integer")?;
                return Ok(Some(format!("{canonical}{index}")));
            }
        }
    }
    if value.is_empty() || value.bytes().any(|c| c.is_ascii_whitespace() || c == 0) {
        bail!("invalid compute device name");
    }
    Ok(Some(value.to_owned()))
}

/// One loaded engine with checkpoint-scoped reference or description conditioning.
/// A description session samples candidates; it does not establish a persistent voice identity.
pub struct Session {
    kind: Backend,
    inner: Inner,
}
enum Inner {
    #[cfg(any(spqx_mlx, feature = "tch-backend"))]
    Native(Box<NativeSession>),
    #[cfg(feature = "qwentts")]
    Ggml(crate::ggml::Session),
}
impl Session {
    /// Load a local model and prepare a reference once for repeated requests.
    pub fn load(
        model: &Path,
        reference: &Path,
        transcript: &str,
        language: &str,
        options: &EngineOptions,
    ) -> Result<Self> {
        if transcript.trim().is_empty() {
            bail!("reference transcript must not be empty");
        }
        let kind = select_backend(model, options.backend)?;
        let inner = match kind {
            #[cfg(any(spqx_mlx, feature = "tch-backend"))]
            Backend::Mlx | Backend::Tch => Inner::Native(Box::new(NativeSession::load(
                model, reference, transcript, language, options,
            )?)),
            #[cfg(feature = "qwentts")]
            Backend::Ggml => Inner::Ggml(crate::ggml::Session::load(
                model, reference, transcript, language, options,
            )?),
            _ => bail!("engine is not compiled in"),
        };
        Ok(Self { kind, inner })
    }
    /// Load a VoiceDesign checkpoint once, without reference audio or a preset speaker.
    /// Repeated synthesis reuses the model and description, not an enrolled identity.
    /// Only the pinned GGML engine implements this operation; no backend/SDK fallback.
    /// A seed is scoped to this engine/checkpoint, not portable voice conditioning.
    pub fn load_voice_design(
        model: &Path,
        description: &str,
        language: &str,
        seed: Option<u64>,
        options: &EngineOptions,
    ) -> Result<Self> {
        let seed = validate_design(description, language, seed)?;
        Self::load_conditioned(
            model,
            language,
            options,
            PreparedInput::Description { description, seed },
        )
    }
    /// Load an exact saved raw speaker vector, without reference encoding or ICL substitution.
    /// The consuming backend/artifacts must match; native validates model width before generation.
    pub fn load_frozen(
        model: &Path,
        frozen: &crate::conditioning::FrozenConditioning,
        language: &str,
        seed: Option<u64>,
        options: &EngineOptions,
    ) -> Result<Self> {
        frozen.validate()?;
        let seed = checked_seed(seed)?;
        Self::load_conditioned(
            model,
            language,
            options,
            PreparedInput::Frozen { frozen, seed },
        )
    }
    fn load_conditioned(
        model: &Path,
        language: &str,
        options: &EngineOptions,
        input: PreparedInput<'_>,
    ) -> Result<Self> {
        let kind = select_backend(model, options.backend)?;
        let inner = match kind {
            #[cfg(feature = "qwentts")]
            Backend::Ggml => Inner::Ggml(match input {
                PreparedInput::Description { description, seed } => {
                    crate::ggml::Session::load_voice_design(
                        model,
                        description,
                        language,
                        seed,
                        options,
                    )?
                }
                PreparedInput::Frozen { frozen, seed } => {
                    crate::ggml::Session::load_frozen(model, frozen, language, seed, options)?
                }
            }),
            _ => bail!(
                "{} is not implemented for {kind:?}; no fallback",
                "requested non-reference conditioning"
            ),
        };
        Ok(Self { kind, inner })
    }
    /// Export the raw speaker vector from native reference preparation, without recognition enrollment.
    pub fn frozen_conditioning(&self) -> Result<crate::conditioning::FrozenConditioning> {
        match &self.inner {
            #[cfg(feature = "qwentts")]
            Inner::Ggml(engine) => engine.frozen_conditioning(),
            #[cfg(any(spqx_mlx, feature = "tch-backend"))]
            _ => bail!("raw conditioning export is not implemented for this engine; no fallback"),
        }
    }
    /// Selected engine name (not a claim that an automatic device was observed).
    pub fn backend_name(&self) -> &'static str {
        match self.kind {
            Backend::Mlx => "mlx",
            Backend::Tch => "tch",
            Backend::Ggml => "ggml",
            Backend::Auto => "auto",
        }
    }
    /// Stream decoded mono f32 PCM. Callbacks may execute on the native worker thread.
    pub fn synthesize<F, C>(
        &self,
        text: &str,
        sampling: &Sampling,
        audio: F,
        cancelled: C,
    ) -> Result<Stop>
    where
        F: FnMut(&[f32], u32) -> bool + Send,
        C: Fn() -> bool + Send + Sync,
    {
        if text.trim().is_empty() {
            bail!("empty synthesis text");
        }
        if sampling.max_new_tokens <= 0
            || sampling.chunk_size == 0
            || !sampling.temperature.is_finite()
            || sampling.temperature < 0.0
        {
            bail!("invalid sampling limits");
        }
        match &self.inner {
            #[cfg(any(spqx_mlx, feature = "tch-backend"))]
            Inner::Native(native) => {
                let mut audio = audio;
                let stop = native.inference.generate_with_icl_session_streaming(
                    &native.reference,
                    text,
                    sampling.temperature,
                    sampling.top_k,
                    sampling.max_new_tokens,
                    sampling.chunk_size,
                    |samples, rate| !cancelled() && audio(samples, rate),
                )?;
                Ok(stop)
            }
            #[cfg(feature = "qwentts")]
            Inner::Ggml(engine) => engine.synthesize(text, sampling, audio, cancelled),
        }
    }
    /// Release native transient caches without initializing an unrelated engine.
    pub fn clear_cache(&self) {
        #[cfg(spqx_mlx)]
        if matches!(&self.inner, Inner::Native(_)) {
            unsafe {
                crate::backend::mlx::ffi::mlx_clear_cache();
            }
        }
    }
}

enum PreparedInput<'a> {
    Description {
        description: &'a str,
        seed: i64,
    },
    Frozen {
        frozen: &'a crate::conditioning::FrozenConditioning,
        seed: i64,
    },
}
fn validate_design(description: &str, language: &str, seed: Option<u64>) -> Result<i64> {
    if description.trim().is_empty() || description.len() > 4096 || description.contains('\0') {
        bail!("VoiceDesign description must be nonempty UTF-8, at most 4096 bytes, without NUL");
    }
    if language.trim().is_empty() || language.contains('\0') {
        bail!("VoiceDesign language must be nonempty and without NUL");
    }
    checked_seed(seed)
}
fn checked_seed(seed: Option<u64>) -> Result<i64> {
    Ok(seed
        .map(i64::try_from)
        .transpose()
        .context("VoiceDesign seed exceeds i64 range")?
        .unwrap_or(-1))
}

#[cfg(any(spqx_mlx, feature = "tch-backend"))]
struct NativeSession {
    inference: crate::inference::TTSInference,
    reference: crate::inference::IclSession,
}
#[cfg(any(spqx_mlx, feature = "tch-backend"))]
impl NativeSession {
    fn load(
        model: &Path,
        reference: &Path,
        transcript: &str,
        language: &str,
        options: &EngineOptions,
    ) -> Result<Self> {
        use crate::{
            audio::{load_wav_file, resample},
            audio_encoder::AudioEncoder,
            speaker_encoder::SpeakerEncoder,
            tensor::Device,
        };
        #[cfg(spqx_mlx)]
        if !["auto", "metal", "gpu"].contains(&options.device.to_lowercase().as_str()) {
            bail!("MLX requires its Metal device; select GGML plus GGUF assets for another device");
        }
        #[cfg(spqx_mlx)]
        crate::backend::mlx::stream::init_mlx(true);
        #[cfg(spqx_mlx)]
        let device = Device::Cpu; // MLX's initialized default stream owns device placement.
        #[cfg(feature = "tch-backend")]
        let device = match options.device.to_lowercase().as_str() {
            "cpu" => Device::Cpu,
            "auto" if !tch::Cuda::is_available() => Device::Cpu,
            "auto" | "cuda" | "cuda:0" => Device::Gpu(0),
            _ => bail!("legacy tch accepts auto, cpu or cuda:0"),
        };
        let inference = crate::inference::TTSInference::new(model, device)?;
        let speaker = SpeakerEncoder::load(
            inference.weights(),
            &inference.config().speaker_encoder_config,
            device,
        )?;
        let encoder =
            AudioEncoder::load(&model.join("speech_tokenizer/model.safetensors"), device)?;
        let (samples, rate) =
            load_wav_file(reference.to_str().context("reference path must be UTF-8")?)?;
        let wanted = inference.config().speaker_encoder_config.sample_rate;
        let samples = if rate == wanted {
            samples
        } else {
            resample(&samples, rate, wanted)?
        };
        let embedding = speaker.extract_embedding(&samples)?;
        let codes = encoder.encode(&samples)?;
        let reference = inference.prepare_icl_session(transcript, &codes, &embedding, language)?;
        Ok(Self {
            inference,
            reference,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn tail_callback_cancellation_overrides_eos_and_cap() {
        assert_eq!(Stop::Eos.after_callback(false), Stop::Cancelled);
        assert_eq!(Stop::CapReached.after_callback(false), Stop::Cancelled);
        assert_eq!(Stop::Eos.after_callback(true), Stop::Eos);
        assert_eq!(Stop::Cancelled.after_callback(true), Stop::Cancelled);
    }
    #[test]
    fn design_validation_is_model_free_and_seed_is_not_silently_changed() {
        assert_eq!(
            validate_design("Adult fictional German voice", "german", Some(4242)).unwrap(),
            4242
        );
        assert_eq!(validate_design("A voice", "auto", None).unwrap(), -1);
        for description in ["", " \n", "bad\0instruction", &"x".repeat(4097)] {
            assert!(validate_design(description, "german", None).is_err());
        }
        assert!(validate_design("A voice", "", None).is_err());
        assert!(validate_design("A voice", "de\0", None).is_err());
        assert!(validate_design("A voice", "german", Some(u64::MAX)).is_err());
    }
    #[test]
    fn devices_are_explicit() {
        assert_eq!(ggml_device("cuda:2").unwrap(), Some("CUDA2".into()));
        assert_eq!(ggml_device("cpu").unwrap(), Some("CPU".into()));
        assert_eq!(ggml_device("auto").unwrap(), None);
        assert!(ggml_device("cuda:-1").is_err());
        assert!(ggml_device("bad device").is_err());
    }
    #[test]
    fn auto_gguf_uses_whole_pipeline_engine() {
        #[cfg(feature = "qwentts")]
        assert_eq!(
            select_backend(Path::new("model.gguf"), Backend::Auto).unwrap(),
            Backend::Ggml
        );
    }
}
