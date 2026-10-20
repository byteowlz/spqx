// Copyright 2026 Claude Code on behalf of Michael Yuan.
// SPDX-License-Identifier: Apache-2.0

//! Local Qwen synthesis with prepared whole-pipeline sessions.
//!
//! MLX is effective on Apple Silicon; the optional qwentts engine loads a pinned
//! GGML runtime for portable desktop/server execution. Device availability,
//! native packaging and speech quality require separate target validation.
//!
//! ```no_run
//! use std::path::Path;
//! use spqx_core::synthesis::{EngineOptions, Sampling, Session};
//! # fn main() -> anyhow::Result<()> {
//! let session = Session::load(Path::new("model.gguf"), Path::new("reference.wav"),
//!     "The matching reference transcript.", "english", &EngineOptions::default())?;
//! let sampling = Sampling { temperature: 0.9, top_k: 50,
//!     max_new_tokens: 384, chunk_size: 4 };
//! let stop = session.synthesize("Hello.", &sampling,
//!     |pcm, rate| { /* deliver mono f32 samples at rate */ true }, || false)?;
//! assert!(!stop.truncated());
//! # Ok(())
//! # }
//! ```
//!
//! CLI and framed worker share this session. Native tensor APIs are available
//! with effective MLX or tch-backend; tensor diagnostic binaries additionally
//! require legacy-tools. Model assets and runtime libraries remain separate.
//! See docs/portable-engines.md for the current qualification matrix.

#![warn(missing_docs)]
#![warn(rustdoc::missing_doc_code_examples)]

// Tensor backends remain mutually exclusive; GGML is a whole-pipeline engine.
#[cfg(all(feature = "tch-backend", spqx_mlx))]
compile_error!("Features 'tch-backend' and effective MLX are mutually exclusive");
#[cfg(not(any(feature = "tch-backend", spqx_mlx, feature = "qwentts")))]
compile_error!("Enable qwentts, tch-backend, or MLX on Apple Silicon");

pub mod audio;
#[cfg(spqx_mlx)]
pub mod backend;
pub mod config;
pub mod error;
#[cfg(feature = "qwentts")]
mod ggml;
pub mod postprocess;
pub mod stdio;
pub mod synthesis;
pub mod worker;

#[cfg(all(feature = "legacy-api", any(spqx_mlx, feature = "tch-backend")))]
pub mod api;
#[cfg(any(spqx_mlx, feature = "tch-backend"))]
pub mod audio_encoder;
#[cfg(any(spqx_mlx, feature = "tch-backend"))]
pub mod inference;
#[cfg(any(spqx_mlx, feature = "tch-backend"))]
pub mod layers;
#[cfg(any(spqx_mlx, feature = "tch-backend"))]
pub mod model;
#[cfg(any(spqx_mlx, feature = "tch-backend"))]
pub mod speaker_encoder;
#[cfg(any(spqx_mlx, feature = "tch-backend"))]
pub mod tensor;
#[cfg(any(spqx_mlx, feature = "tch-backend"))]
pub mod tokenizer;
#[cfg(any(spqx_mlx, feature = "tch-backend"))]
pub mod trace;
#[cfg(any(spqx_mlx, feature = "tch-backend"))]
pub mod types;
#[cfg(any(spqx_mlx, feature = "tch-backend"))]
pub mod vocoder;
#[cfg(any(spqx_mlx, feature = "tch-backend"))]
pub mod weights;

pub use audio::AudioInput;
pub use config::{GenerationConfig, Qwen3TTSConfig, TTSModelType, TokenizerType};
pub use error::{Qwen3TTSError, Result};
#[cfg(any(spqx_mlx, feature = "tch-backend"))]
pub use model::{GenerationParams, Qwen3TTSModel};
#[cfg(any(spqx_mlx, feature = "tch-backend"))]
pub use tokenizer::Qwen3TTSTokenizer;
#[cfg(any(spqx_mlx, feature = "tch-backend"))]
pub use types::{
    DecodedAudio, EncodedAudio, GenerationOutput, Language, Speaker, VoiceClonePromptItem,
    VoiceInstruction,
};

/// Crate version.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Default output sample rate in Hz.
pub const DEFAULT_SAMPLE_RATE: u32 = 24000;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_version() {
        assert!(!VERSION.is_empty());
    }

    #[test]
    fn test_default_sample_rate() {
        assert_eq!(DEFAULT_SAMPLE_RATE, 24000);
    }

    #[cfg(any(spqx_mlx, feature = "tch-backend"))]
    #[test]
    fn test_language_conversion() {
        // Test that "english" and "EN" both convert to Language::English
        let lang: Language = "english".into();
        assert_eq!(lang.as_str(), "english");

        let lang: Language = "EN".into();
        assert_eq!(lang.as_str(), "english");

        let lang: Language = "Auto".into();
        assert_eq!(lang.as_str(), "auto");

        let lang: Language = "zh".into();
        assert_eq!(lang.as_str(), "chinese");

        let lang = Language::Auto;
        assert_eq!(lang.as_str(), "auto");
    }

    #[cfg(any(spqx_mlx, feature = "tch-backend"))]
    #[test]
    fn test_speaker_creation() {
        let speaker = Speaker::new("Vivian");
        assert_eq!(speaker.name(), "Vivian");

        let speaker: Speaker = "John".into();
        assert_eq!(speaker.name(), "John");
    }

    #[test]
    fn test_audio_input_from_string() {
        let input = AudioInput::from("test.wav");
        match input {
            AudioInput::FilePath(path) => assert_eq!(path, "test.wav"),
            _ => panic!("Expected FilePath"),
        }
    }
}
