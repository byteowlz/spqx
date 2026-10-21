//! Complete ICL reference context, deliberately separate from continuous speaker geometry.
use crate::conditioning::{hash_bytes, ConditioningBackend, ConditioningScope, FrozenConditioning};
use anyhow::{ensure, Result};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Qualified codec artifact, not an inference from equal dimensions or a model family name.
pub const QWEN_12HZ_CODEC: &str =
    "b16b95557c7c7340a121757bd6855b9609e1cf4ad3fad0778b89393293ae5f3d";
/// Closed format limits the native pointer's readable shape and embedding indices.
pub const CODEBOOKS: usize = 16;
/// Number of entries in each qualified RVQ codebook, excluding Talker control tokens.
pub const CODEBOOK_SIZE: i32 = 2048;
/// Bounded reference duration (512/12.5 seconds), not the generated utterance cap.
pub const MAX_REFERENCE_FRAMES: usize = 512;
const FORMAT: &str = "qwen3-12hz-k16-v2048-k-major-v1";

/// Scoped discrete reference codes and their original text; never an audition transcript.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct IclContext {
    /// Context schema version, distinct from the existing speaker-vector schema.
    pub version: u32,
    /// Exact source/consumer model, codec and encoder space.
    pub scope: ConditioningScope,
    /// Native [codebook, frame] row-major, frame index fastest; no transposition on reload.
    pub format: String,
    /// K in the native [K,T] code matrix.
    pub codebooks: usize,
    /// T in the native [K,T] code matrix.
    pub frames: usize,
    /// Exact discrete codec indices; never normalized or PCA projected.
    pub codes: Vec<i32>,
    /// Original reference transcript, not a later target utterance.
    pub text: String,
    /// Provenance only: the source WAV is not a runtime dependency.
    pub source_audio_sha256: String,
    /// Binds scope, shape, exact codes, text and source hash; not an authorization signature.
    pub sha256: String,
}
impl IclContext {
    /// Validate a native copy and bind its exact payload to a custody checksum.
    pub fn from_native(
        scope: ConditioningScope,
        frames: usize,
        codes: Vec<i32>,
        text: String,
        source_audio_sha256: String,
    ) -> Result<Self> {
        let mut value = Self {
            version: 1,
            scope,
            format: FORMAT.into(),
            codebooks: CODEBOOKS,
            frames,
            codes,
            text,
            source_audio_sha256,
            sha256: String::new(),
        };
        value.validate_payload()?;
        value.sha256 = value.digest()?;
        Ok(value)
    }
    fn validate_payload(&self) -> Result<()> {
        self.scope.validate()?;
        ensure!(
            self.version == 1 && self.format == FORMAT,
            "unsupported ICL context version/layout"
        );
        ensure!(self.scope.backend==ConditioningBackend::Ggml && self.scope.codec_sha256==QWEN_12HZ_CODEC,"ICL context requires the qualified native Qwen12Hz codec; no codec/backend substitution");
        ensure!(
            self.codebooks == CODEBOOKS && (1..=MAX_REFERENCE_FRAMES).contains(&self.frames),
            "invalid ICL codebook/frame shape"
        );
        ensure!(
            self.codes.len() == self.codebooks * self.frames
                && self.codes.iter().all(|v| (0..CODEBOOK_SIZE).contains(v)),
            "invalid ICL code count/range"
        );
        ensure!(
            !self.text.trim().is_empty() && self.text.len() <= 16384 && !self.text.contains('\0'),
            "invalid original ICL reference text"
        );
        ensure!(
            self.source_audio_sha256.len() == 64
                && self
                    .source_audio_sha256
                    .bytes()
                    .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()),
            "invalid ICL source hash"
        );
        Ok(())
    }
    fn digest(&self) -> Result<String> {
        let mut payload = self.clone();
        payload.sha256.clear();
        Ok(hash_bytes(&serde_json::to_vec(&payload)?))
    }
    /// Reject unsupported shapes/artifacts and changed code/text custody before native access.
    pub fn validate(&self) -> Result<()> {
        self.validate_payload()?;
        ensure!(
            self.sha256 == self.digest()?,
            "ICL context codes/text/scope changed"
        );
        Ok(())
    }
}
/// Explicit complete prompt. Existing FrozenConditioning v1 remains vector-only and unchanged.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FrozenIcl {
    /// Complete-prompt schema version.
    pub version: u32,
    /// Explicit distinction from raw-vector-only files.
    pub mode: IclMode,
    /// Complete effective continuous speaker conditioning, including any PCA recipe.
    pub speaker: FrozenConditioning,
    /// Independently preserved discrete reference context.
    pub context: IclContext,
}
/// Serialized mode discriminator; never guessed from optional filenames.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum IclMode {
    /// Speaker vector plus original reference codes and text.
    FullIcl,
}
impl FrozenIcl {
    /// Combine only identical scopes; no conversion, re-extraction or normalization.
    pub fn new(speaker: FrozenConditioning, context: IclContext) -> Result<Self> {
        let prompt = Self {
            version: 1,
            mode: IclMode::FullIcl,
            speaker,
            context,
        };
        prompt.validate()?;
        Ok(prompt)
    }
    /// Validate complete-prompt version, context custody and speaker/context compatibility.
    pub fn validate(&self) -> Result<()> {
        ensure!(self.version == 1, "unsupported frozen ICL prompt version");
        self.context.validate()?;
        self.speaker.require_scope(&self.context.scope)
    }
    /// Keep the complete discrete context unchanged; only same-scope continuous geometry may vary.
    pub fn with_speaker(self, speaker: FrozenConditioning) -> Result<Self> {
        Self::new(speaker, self.context)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> FrozenIcl {
        let scope = ConditioningScope {
            backend: ConditioningBackend::Ggml,
            model_sha256: "a".repeat(64),
            codec_sha256: QWEN_12HZ_CODEC.into(),
            encoder: "unit-test".into(),
            dimension: 3,
            representation: "raw-f32".into(),
        };
        let speaker = FrozenConditioning {
            version: 1,
            scope: scope.clone(),
            values: vec![1., -0., 3.],
            recipe: None,
        };
        FrozenIcl::new(
            speaker,
            IclContext::from_native(
                scope,
                2,
                (0..32).collect(),
                "Original English reference.".into(),
                "c".repeat(64),
            )
            .unwrap(),
        )
        .unwrap()
    }
    #[test]
    fn full_icl_roundtrip_keeps_speaker_bits_codes_and_original_text() {
        let original = fixture();
        let bytes = serde_json::to_vec(&original).unwrap();
        let restored: FrozenIcl = serde_json::from_slice(&bytes).unwrap();
        restored.validate().unwrap();
        assert_eq!(
            restored
                .speaker
                .values
                .iter()
                .map(|x| x.to_bits())
                .collect::<Vec<_>>(),
            original
                .speaker
                .values
                .iter()
                .map(|x| x.to_bits())
                .collect::<Vec<_>>()
        );
        assert_eq!(restored.context.codes, original.context.codes);
        assert_eq!(restored.context.text, original.context.text);
        assert!(serde_json::from_slice::<FrozenConditioning>(&bytes).is_err());
    }
    #[test]
    fn edits_preserve_context_but_reject_incompatible_speaker_space() {
        let prompt = fixture();
        let hash = prompt.context.sha256.clone();
        let mut edited = prompt.speaker.clone();
        edited.values[0] += 0.2;
        let updated = prompt.with_speaker(edited).unwrap();
        assert_eq!(updated.context.sha256, hash);
        let mut incompatible = updated.speaker.clone();
        incompatible.scope.model_sha256 = "d".repeat(64);
        assert!(updated.with_speaker(incompatible).is_err());
    }
    #[test]
    fn malformed_or_tampered_context_fails_without_native_calls() {
        let original = fixture();
        let corruptions: [fn(&mut IclContext); 10] = [
            |x| x.codes[0] = -1,
            |x| x.codes[0] = 2048,
            |x| {
                x.codes.pop();
            },
            |x| x.frames = 0,
            |x| x.frames = 513,
            |x| x.codebooks = 15,
            |x| x.text.push('!'),
            |x| x.text = "bad\0text".into(),
            |x| x.format = "frame-major".into(),
            |x| x.scope.codec_sha256 = "d".repeat(64),
        ];
        for change in corruptions {
            let mut context = original.context.clone();
            change(&mut context);
            assert!(context.validate().is_err());
        }
        let mut wrong_version = original;
        wrong_version.version = 2;
        assert!(wrong_version.validate().is_err());
    }
}
