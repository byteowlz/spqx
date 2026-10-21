//! Model-scoped frozen TTS conditioning and residual-preserving PCA. Never recognition vectors.
use anyhow::{bail, ensure, Context, Result};
use nalgebra::DMatrix;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, fs::File, io::Read, path::Path};

/// Resolved engine namespace for conditioning, never an automatic selection request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ConditioningBackend {
    /// Portable GGML/libqwen space.
    Ggml,
    /// Apple MLX space; data support is not runtime qualification.
    Mlx,
}

/// Fingerprints the consuming model and encoder implementation, not just vector width.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ConditioningScope {
    /// Engine that produced/consumes this space; never inferred from dimension.
    pub backend: ConditioningBackend,
    /// Exact consuming talker/model artifact SHA256.
    pub model_sha256: String,
    /// Exact codec/tokenizer artifact SHA256.
    pub codec_sha256: String,
    /// Versioned speaker-encoder implementation identity.
    pub encoder: String,
    /// Number of raw speaker conditioning values.
    pub dimension: usize,
    /// Currently raw-f32 only; no implicit L2 normalization or casting.
    pub representation: String,
}
impl ConditioningScope {
    /// Reject incomplete or unsupported scope declarations.
    pub fn validate(&self) -> Result<()> {
        ensure!(
            is_hash(&self.model_sha256) && is_hash(&self.codec_sha256),
            "invalid model/codec fingerprint"
        );
        ensure!(
            (2..=8192).contains(&self.dimension),
            "invalid conditioning dimension"
        );
        ensure!(
            !self.encoder.is_empty() && self.encoder.len() <= 256,
            "invalid encoder identity"
        );
        ensure!(
            self.representation == "raw-f32",
            "unsupported normalization/representation; no conversion"
        );
        Ok(())
    }
}
/// Reopenable editor settings, subordinate to the fully frozen vector.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PcaRecipe {
    /// Content identity of the exact fitted PCA bank.
    pub space: String,
    /// Source label in that bank, not a canonical identity.
    pub base: String,
    /// Offsets in this fitted bank's sample standard deviations, not semantic traits.
    pub offsets: Vec<f64>,
}
/// The complete vector consumed by synthesis; a recipe alone is not the saved voice.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FrozenConditioning {
    /// Frozen-conditioning schema version (1).
    pub version: u32,
    /// Exact compatible model/encoder space.
    pub scope: ConditioningScope,
    /// Effective complete f32 vector consumed by this engine.
    pub values: Vec<f32>,
    /// Optional PCA editor state, not a regeneration substitute.
    pub recipe: Option<PcaRecipe>,
}
impl FrozenConditioning {
    /// Validate version, bounded finite raw vector and optional recipe structure.
    pub fn validate(&self) -> Result<()> {
        ensure!(self.version == 1, "unsupported frozen conditioning version");
        self.scope.validate()?;
        validate_vector(&self.values, self.scope.dimension)?;
        if let Some(recipe) = &self.recipe {
            ensure!(
                is_hash(&recipe.space) && valid_name(&recipe.base),
                "invalid PCA recipe identity"
            );
            ensure!(
                !recipe.offsets.is_empty()
                    && recipe.offsets.len() <= 8
                    && recipe.offsets.iter().all(|v| v.is_finite()),
                "invalid PCA offsets"
            );
        }
        Ok(())
    }
    /// Fail explicitly on any incompatible artifact, encoder or backend.
    pub fn require_scope(&self, expected: &ConditioningScope) -> Result<()> {
        self.validate()?;
        ensure!(&self.scope == expected, "incompatible conditioning space; no normalization, padding, re-extraction or backend fallback");
        Ok(())
    }
}
/// A fitted, checkpoint-scoped voice bank. No neural weight training or enrollment.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PcaSpace {
    /// PCA bank schema version (1).
    pub version: u32,
    /// One shared space; mixed-model banks are rejected.
    pub scope: ConditioningScope,
    /// Independent source vectors, in reproducible label order.
    pub sources: BTreeMap<String, Vec<f32>>,
    /// Centered-SVD principal directions, rows orthonormal.
    pub components: Vec<Vec<f64>>,
    /// Sample standard deviations along fitted directions.
    pub scales: Vec<f64>,
    /// Explicit experimental bound, not a learned acoustic safety/naturalness range.
    pub max_offset: f64,
}
impl PcaSpace {
    /// Fit actual centered PCA; rank is bounded by independent sources minus one.
    pub fn fit(
        scope: ConditioningScope,
        sources: BTreeMap<String, Vec<f32>>,
        count: usize,
        max_offset: f64,
    ) -> Result<Self> {
        scope.validate()?;
        ensure!(
            (2..=128).contains(&sources.len()),
            "PCA requires 2..128 independent source vectors"
        );
        ensure!(
            count > 0 && count <= 8 && count < sources.len() && count <= scope.dimension,
            "invalid PCA component count"
        );
        for (name, vector) in &sources {
            ensure!(valid_name(name), "invalid PCA source name");
            validate_vector(vector, scope.dimension)?;
        }
        let n = sources.len();
        let vectors: Vec<_> = sources.values().collect();
        let means: Vec<_> = (0..scope.dimension)
            .map(|j| vectors.iter().map(|v| v[j] as f64).sum::<f64>() / n as f64)
            .collect();
        let centered = DMatrix::from_fn(n, scope.dimension, |i, j| vectors[i][j] as f64 - means[j]);
        let svd = centered.svd(false, true);
        let threshold = svd.singular_values[0] * 1e-10;
        ensure!(
            svd.singular_values[count - 1] > threshold && svd.singular_values[count - 1] > 1e-12,
            "degenerate PCA bank/rank"
        );
        let vt = svd.v_t.context("PCA solver returned no directions")?;
        let mut components: Vec<Vec<f64>> = (0..count)
            .map(|i| vt.row(i).iter().copied().collect())
            .collect();
        // Resolve sign ambiguity deterministically; this does not give an axis semantic meaning.
        for row in &mut components {
            let pivot = row
                .iter()
                .enumerate()
                .max_by(|a, b| a.1.abs().total_cmp(&b.1.abs()))
                .unwrap()
                .0;
            if row[pivot] < 0.0 {
                for v in row {
                    *v = -*v;
                }
            }
        }
        let result = Self {
            version: 1,
            scope,
            sources,
            components,
            scales: svd
                .singular_values
                .iter()
                .take(count)
                .map(|v| v / ((n - 1) as f64).sqrt())
                .collect(),
            max_offset,
        };
        result.validate()?;
        Ok(result)
    }
    /// Validate finite sources, dimensions, scales and orthonormal directions.
    pub fn validate(&self) -> Result<()> {
        self.scope.validate()?;
        ensure!(
            self.version == 1 && (2..=128).contains(&self.sources.len()),
            "invalid PCA bank/version"
        );
        ensure!(
            !self.components.is_empty()
                && self.components.len() <= 8
                && self.components.len() < self.sources.len(),
            "invalid PCA rank"
        );
        ensure!(
            self.scales.len() == self.components.len()
                && self.scales.iter().all(|v| v.is_finite() && *v > 0.0),
            "invalid PCA scales"
        );
        ensure!(
            self.max_offset.is_finite() && self.max_offset > 0.0 && self.max_offset <= 3.0,
            "invalid experimental PCA bound"
        );
        for (name, vector) in &self.sources {
            ensure!(valid_name(name), "invalid source name");
            validate_vector(vector, self.scope.dimension)?;
        }
        for (i, row) in self.components.iter().enumerate() {
            ensure!(
                row.len() == self.scope.dimension && row.iter().all(|v| v.is_finite()),
                "invalid PCA direction"
            );
            for (j, other) in self.components.iter().take(i + 1).enumerate() {
                let dot: f64 = row.iter().zip(other).map(|(a, b)| a * b).sum();
                ensure!(
                    (dot - if i == j { 1.0 } else { 0.0 }).abs() < 1e-6,
                    "PCA directions are not orthonormal"
                );
            }
        }
        Ok(())
    }
    /// Stable SHA256 of the canonical serialized fitted bank.
    pub fn id(&self) -> Result<String> {
        self.validate()?;
        Ok(hash_bytes(&serde_json::to_vec(self)?))
    }
    /// Fitted standardized coordinates for display; these are measured, not pitch/age labels.
    pub fn coordinates(&self, values: &[f32]) -> Result<Vec<f64>> {
        self.validate()?;
        validate_vector(values, self.scope.dimension)?;
        let means: Vec<f64> = (0..self.scope.dimension)
            .map(|j| {
                self.sources.values().map(|v| v[j] as f64).sum::<f64>() / self.sources.len() as f64
            })
            .collect();
        Ok(self
            .components
            .iter()
            .zip(&self.scales)
            .map(|(row, scale)| {
                values
                    .iter()
                    .zip(&means)
                    .zip(row)
                    .map(|((v, m), c)| (*v as f64 - m) * c)
                    .sum::<f64>()
                    / scale
            })
            .collect())
    }
    /// Apply standardized direction offsets, keeping all unedited residual information.
    pub fn edit(&self, base: &str, offsets: &[f64]) -> Result<FrozenConditioning> {
        self.validate()?;
        let source = self.sources.get(base).context("unknown PCA source")?;
        ensure!(
            offsets.len() == self.components.len()
                && offsets
                    .iter()
                    .all(|v| v.is_finite() && v.abs() <= self.max_offset),
            "invalid/out-of-range PCA offsets"
        );
        // Offset directions, NEVER low-rank reconstruction: keep the base's residual.
        let values = if offsets.iter().all(|v| *v == 0.0) {
            source.clone()
        } else {
            source
                .iter()
                .enumerate()
                .map(|(j, value)| {
                    (*value as f64
                        + offsets
                            .iter()
                            .enumerate()
                            .map(|(i, d)| d * self.scales[i] * self.components[i][j])
                            .sum::<f64>()) as f32
                })
                .collect()
        };
        let frozen = FrozenConditioning {
            version: 1,
            scope: self.scope.clone(),
            values,
            recipe: Some(PcaRecipe {
                space: self.id()?,
                base: base.into(),
                offsets: offsets.to_vec(),
            }),
        };
        frozen.validate()?;
        Ok(frozen)
    }
}

/// SHA256 content identity, not authorization or a speaker identity.
pub fn hash_bytes(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
/// Streaming artifact hash; never reads a whole model into host memory.
pub fn hash_file(path: &Path) -> Result<String> {
    let mut file = File::open(path).with_context(|| format!("opening {}", path.display()))?;
    let mut digest = Sha256::new();
    let mut buffer = [0u8; 65536];
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        digest.update(&buffer[..count]);
    }
    Ok(format!("{:x}", digest.finalize()))
}
fn is_hash(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
fn valid_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && !["constructor", "prototype", "__proto__"].contains(&value)
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}
fn validate_vector(vector: &[f32], dimension: usize) -> Result<()> {
    ensure!(
        vector.len() == dimension && vector.iter().all(|v| v.is_finite() && v.abs() <= 1e4),
        "invalid conditioning dimensions/values"
    );
    if !vector.iter().any(|v| *v != 0.0) {
        bail!("zero conditioning vector");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn scope() -> ConditioningScope {
        ConditioningScope {
            backend: ConditioningBackend::Ggml,
            model_sha256: "a".repeat(64),
            codec_sha256: "b".repeat(64),
            encoder: "synthetic-test-only".into(),
            dimension: 3,
            representation: "raw-f32".into(),
        }
    }
    fn space() -> PcaSpace {
        PcaSpace::fit(
            scope(),
            [
                ("A".into(), vec![2., 0., 7.]),
                ("C".into(), vec![0., 2., 7.]),
                ("D".into(), vec![-2., 0., 7.]),
            ]
            .into(),
            2,
            0.75,
        )
        .unwrap()
    }
    #[test]
    fn neutral_is_exact_and_residual_is_preserved() {
        let space = space();
        let zero = space.edit("A", &[0., 0.]).unwrap();
        assert_eq!(
            zero.values.iter().map(|v| v.to_bits()).collect::<Vec<_>>(),
            space.sources["A"]
                .iter()
                .map(|v| v.to_bits())
                .collect::<Vec<_>>()
        );
        let edited = space.edit("A", &[0.3, -0.2]).unwrap();
        assert_eq!(edited.values[2], 7.);
        for i in 0..2 {
            let delta: f64 = edited
                .values
                .iter()
                .zip(&zero.values)
                .zip(&space.components[i])
                .map(|((a, b), axis)| (*a as f64 - *b as f64) * axis)
                .sum();
            assert!((delta - [0.3, -0.2][i] * space.scales[i]).abs() < 1e-6);
        }
    }
    #[test]
    fn freeze_roundtrip_and_scope_mismatch_are_explicit() {
        let original = space().edit("C", &[0.75, 0.]).unwrap();
        let reloaded: FrozenConditioning =
            serde_json::from_slice(&serde_json::to_vec(&original).unwrap()).unwrap();
        assert_eq!(reloaded.values, original.values);
        reloaded.require_scope(&scope()).unwrap();
        let mut wrong = scope();
        wrong.model_sha256 = "c".repeat(64);
        assert!(reloaded.require_scope(&wrong).is_err());
        wrong = scope();
        wrong.backend = ConditioningBackend::Mlx;
        assert!(reloaded.require_scope(&wrong).is_err());
    }
    #[test]
    fn invalid_values_axes_and_degenerate_banks_fail() {
        let space = space();
        for offsets in [vec![f64::NAN, 0.], vec![0.751, 0.], vec![0.]] {
            assert!(space.edit("A", &offsets).is_err());
        }
        assert!(space.edit("unknown", &[0., 0.]).is_err());
        assert!(PcaSpace::fit(
            scope(),
            [
                ("A".into(), vec![1., 2., 3.]),
                ("B".into(), vec![1., 2., 3.])
            ]
            .into(),
            1,
            0.75
        )
        .is_err());
        let mut bad = space.edit("A", &[0., 0.]).unwrap();
        bad.values[0] = f32::NAN;
        assert!(bad.validate().is_err());
        bad.values = vec![1.];
        assert!(bad.validate().is_err());
        let mut badspace = space.clone();
        badspace.components[0][0] += 1.;
        assert!(badspace.validate().is_err());
    }
}
