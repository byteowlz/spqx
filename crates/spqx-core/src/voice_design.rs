//! Model-free VoiceDesign prompt layout, matching the official Qwen3-TTS
//! prefill: instruction text, role prefix, then a codec prefix with NO speaker
//! position (no preset ID, x-vector or reference codes). Target text is fed by
//! the engine using the official streaming text input.
#![cfg_attr(not(spqx_mlx), allow(dead_code))]
use crate::{
    config::Qwen3TTSConfig,
    error::{Qwen3TTSError, Result},
};

/// Official VoiceDesign checkpoint generation_config repetition penalty.
pub(crate) const REPETITION_PENALTY: f64 = 1.05;

pub(crate) fn check_model(config: &Qwen3TTSConfig) -> Result<()> {
    match config.tts_model_type.as_deref() {
        Some("voice_design") => Ok(()),
        other => Err(Qwen3TTSError::InvalidInput(format!(
            "VoiceDesign requires a voice_design checkpoint, loaded {}; no Base/preset substitution",
            other.unwrap_or("unknown")
        ))),
    }
}

/// Request-independent token layout for one description.
pub(crate) struct Prompt {
    /// `<|im_start|>user\n{description}<|im_end|>\n`, text-only conditioning.
    pub instruction: Vec<i64>,
    /// `<|im_start|>assistant\n`.
    pub role: [i64; 3],
    /// Think/language tags, then pad and bos. Never a speaker slot.
    pub codec_prefix: Vec<i64>,
}

impl Prompt {
    /// `framing` is the tokenizer's (`user`, newline) IDs.
    pub(crate) fn new(
        config: &Qwen3TTSConfig,
        description: Vec<i64>,
        framing: (i64, i64),
        language: &str,
    ) -> Result<Self> {
        check_model(config)?;
        if description.is_empty() || description.len() > 4096 {
            return Err(Qwen3TTSError::InvalidInput(
                "VoiceDesign description must tokenize to 1..=4096 tokens".into(),
            ));
        }
        let (user, newline) = framing;
        let mut instruction = vec![config.im_start_token_id as i64, user, newline];
        instruction.extend(description);
        instruction.extend([config.im_end_token_id as i64, newline]);
        let talker = &config.talker_config;
        let mut codec_prefix = if language.eq_ignore_ascii_case("auto") {
            vec![
                talker.codec_nothink_id as i64,
                talker.codec_think_bos_id as i64,
            ]
        } else {
            let id = talker
                .codec_language_id
                .as_ref()
                .and_then(|ids| ids.get(&language.to_lowercase()))
                .ok_or_else(|| {
                    Qwen3TTSError::InvalidInput(format!(
                        "unsupported VoiceDesign language: {language}"
                    ))
                })?;
            vec![
                talker.codec_think_id as i64,
                talker.codec_think_bos_id as i64,
                i64::from(*id),
            ]
        };
        codec_prefix.extend([
            talker.codec_think_eos_id as i64,
            talker.codec_pad_id as i64,
            talker.codec_bos_id as i64,
        ]);
        Ok(Self {
            instruction,
            role: [
                config.im_start_token_id as i64,
                config.assistant_token_id as i64,
                newline,
            ],
            codec_prefix,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn design_config() -> Qwen3TTSConfig {
        let mut config = Qwen3TTSConfig {
            tts_model_type: Some("voice_design".into()),
            ..Qwen3TTSConfig::default()
        };
        config.talker_config.codec_language_id = Some([("german".into(), 2056)].into());
        config
    }
    #[test]
    fn description_prefix_is_text_only_and_codec_has_no_speaker() {
        let config = design_config();
        let t = &config.talker_config;
        let prompt = Prompt::new(&config, vec![81, 82], (872, 198), "German").unwrap();
        assert_eq!(
            prompt.instruction,
            [
                config.im_start_token_id as i64,
                872,
                198,
                81,
                82,
                config.im_end_token_id as i64,
                198
            ]
        );
        assert_eq!(
            prompt.role,
            [
                config.im_start_token_id as i64,
                config.assistant_token_id as i64,
                198
            ]
        );
        assert_eq!(
            prompt.codec_prefix,
            [
                t.codec_think_id as i64,
                t.codec_think_bos_id as i64,
                2056,
                t.codec_think_eos_id as i64,
                t.codec_pad_id as i64,
                t.codec_bos_id as i64
            ]
        );
        let auto = Prompt::new(&config, vec![81], (872, 198), "auto").unwrap();
        assert_eq!(
            auto.codec_prefix,
            [
                t.codec_nothink_id as i64,
                t.codec_think_bos_id as i64,
                t.codec_think_eos_id as i64,
                t.codec_pad_id as i64,
                t.codec_bos_id as i64
            ]
        );
    }
    #[test]
    fn wrong_checkpoint_language_and_empty_description_fail_closed() {
        let mut config = design_config();
        assert!(Prompt::new(&config, vec![81], (872, 198), "klingon").is_err());
        assert!(Prompt::new(&config, vec![], (872, 198), "german").is_err());
        assert!(Prompt::new(&config, vec![1; 4097], (872, 198), "german").is_err());
        for kind in [None, Some("base"), Some("custom_voice")] {
            config.tts_model_type = kind.map(Into::into);
            assert!(Prompt::new(&config, vec![81], (872, 198), "german").is_err());
        }
    }
}
