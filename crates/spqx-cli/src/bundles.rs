//! Saved audition + exact conditioning. Canonical identity belongs to voqx, not this module.
use anyhow::{bail, ensure, Context, Result};
use serde::{Deserialize, Serialize};
use spqx_core::conditioning::{hash_bytes, FrozenConditioning, PcaSpace};
use std::{
    collections::BTreeMap,
    fs,
    io::{Cursor, Write},
    path::{Component, Path, PathBuf},
};
use voqx_core::registry_paths::RegistryPaths;
use voqx_core::{RegistryStore, SourceRecording, SynthesisProfile, SynthesisReference};

pub const ENGINE: &str = "spqx-qwen3";
pub const TAG: &str = "spqx:conditioning-v1";
pub const ICL_TAG: &str = "spqx:conditioning-v2";

#[cfg(test)]
#[path = "bundles_icl_tests.rs"]
mod icl_tests;
#[cfg(test)]
#[path = "bundles_tests.rs"]
mod tests;

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct VoiceBundle {
    pub version: u32,
    pub conditioning: FrozenConditioning,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icl_context: Option<spqx_core::icl::IclContext>,
    pub pca: Option<PcaSpace>,
    pub text: String,
    pub language: String,
    pub seed: Option<u64>,
    pub reference_sha256: String,
    pub provenance: BTreeMap<String, String>,
}
impl VoiceBundle {
    pub fn validate(&self, wave: &[u8]) -> Result<f64> {
        ensure!(
            matches!(
                (self.version, self.icl_context.is_some()),
                (1, false) | (2, true)
            ),
            "unsupported/mismatched voice bundle conditioning mode"
        );
        self.conditioning.validate()?;
        if let Some(context) = &self.icl_context {
            context.validate()?;
            self.conditioning.require_scope(&context.scope)?;
        }
        ensure!(
            !self.text.trim().is_empty() && self.text.len() <= 16384 && !self.text.contains('\0'),
            "invalid audition transcript"
        );
        ensure!(
            !self.language.trim().is_empty()
                && self.language.len() <= 64
                && !self.language.contains('\0'),
            "invalid language"
        );
        ensure!(
            hash_bytes(wave) == self.reference_sha256,
            "audition hash changed"
        );
        if let Some(recipe) = &self.conditioning.recipe {
            let pca = self
                .pca
                .as_ref()
                .context("edited voice must retain its PCA bank for reopening the controls")?;
            ensure!(pca.id()? == recipe.space, "PCA space changed");
            let expected = pca.edit(&recipe.base, &recipe.offsets)?;
            ensure!(
                expected.scope == self.conditioning.scope
                    && expected.values == self.conditioning.values,
                "saved settings do not describe this frozen vector"
            );
        } else {
            ensure!(self.pca.is_none(), "unexpected PCA bank without a recipe");
        }
        ensure!(
            self.provenance.len() <= 32
                && self
                    .provenance
                    .iter()
                    .all(|(k, v)| k.len() <= 128 && v.len() <= 4096),
            "provenance too large"
        );
        validate_wave(wave)
    }
    pub fn load(directory: &Path) -> Result<(Self, Vec<u8>)> {
        let metadata = read_asset(directory, "voice.json", 16 * 1024 * 1024)?;
        let voice: Self = serde_json::from_slice(&metadata)?;
        let wave = read_asset(directory, "reference.wav", 3_000_000)?;
        let text = read_asset(directory, "reference.txt", 16384)?;
        ensure!(
            text == voice.text.as_bytes(),
            "saved audition transcript changed"
        );
        voice.validate(&wave)?;
        Ok((voice, wave))
    }
    /// Exclusive directory allocation; the validated manifest is the final commit marker.
    /// A crash before it leaves an invalid, never-promoted draft, not a playable saved voice.
    pub fn save(&self, directory: &Path, wave: &[u8]) -> Result<()> {
        self.validate(wave)?;
        if let Some(parent) = directory.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::create_dir(directory).context("draft directory already exists or cannot be created")?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(directory, fs::Permissions::from_mode(0o700))?;
        }
        write_new(&directory.join("reference.wav"), wave)?;
        write_new(&directory.join("reference.txt"), self.text.as_bytes())?;
        write_new(
            &directory.join("voice.json"),
            &serde_json::to_vec_pretty(self)?,
        )?;
        Ok(())
    }
}
fn write_new(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    Ok(())
}
fn read_asset(root: &Path, name: &str, max: u64) -> Result<Vec<u8>> {
    let path = root.join(name);
    let metadata = fs::symlink_metadata(&path)?;
    ensure!(
        metadata.is_file() && !metadata.file_type().is_symlink() && metadata.len() <= max,
        "invalid/oversized/symlink bundle asset"
    );
    ensure!(
        path.canonicalize()?.starts_with(root.canonicalize()?),
        "bundle asset escaped root"
    );
    Ok(fs::read(path)?)
}
fn validate_wave(wave: &[u8]) -> Result<f64> {
    ensure!(wave.len() <= 3_000_000, "audition too large");
    let mut reader = hound::WavReader::new(Cursor::new(wave))?;
    let spec = reader.spec();
    ensure!(
        spec.channels == 1 && spec.sample_rate == 24000,
        "audition must be native mono24k WAV"
    );
    let samples: Vec<f32> = match (spec.sample_format, spec.bits_per_sample) {
        (hound::SampleFormat::Float, 32) => reader
            .samples::<f32>()
            .collect::<std::result::Result<_, _>>()?,
        (hound::SampleFormat::Int, 16) => reader
            .samples::<i16>()
            .map(|v| v.map(|n| n as f32 / 32768.))
            .collect::<std::result::Result<_, _>>()?,
        _ => bail!("unsupported audition sample format"),
    };
    ensure!(
        (24000..=720000).contains(&samples.len()),
        "audition must be 1..30 seconds"
    );
    ensure!(
        samples.iter().all(|v| v.is_finite() && v.abs() <= 1.),
        "nonfinite/out-of-range audition"
    );
    ensure!(
        samples.iter().map(|v| (*v as f64).powi(2)).sum::<f64>() / samples.len() as f64 > 1e-10,
        "silent audition"
    );
    Ok(samples.len() as f64 / 24000.)
}
pub fn store(root: Option<&Path>) -> Result<RegistryStore> {
    Ok(match root {
        Some(root) => RegistryStore::new(RegistryPaths::from_root(root.to_owned())),
        None => RegistryStore::from_default()?,
    })
}
fn safe_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}
fn asset_path(root: &Path, value: &str) -> Result<PathBuf> {
    let path = Path::new(value);
    ensure!(
        !value.contains('\\') && path.components().all(|v| matches!(v, Component::Normal(_))),
        "invalid registry-relative asset path"
    );
    let path = root.join(path).canonicalize()?;
    ensure!(
        path.starts_with(root.canonicalize()?),
        "registry asset escaped root"
    );
    Ok(path)
}
// Test convenience for a fresh, non-retry save intent.
#[cfg(test)]
fn promote(
    draft: &Path,
    name: &str,
    voice_id: Option<&str>,
    registry_root: Option<&Path>,
) -> Result<String> {
    promote_once(draft, name, voice_id, registry_root, None)
}
/// Durable idempotent save operation; operation identity is not a display-name/embedding match.
pub fn promote_once(
    draft: &Path,
    name: &str,
    voice_id: Option<&str>,
    registry_root: Option<&Path>,
    operation_id: Option<&str>,
) -> Result<String> {
    let operation_tag = operation_id
        .map(|id| {
            ensure!(
                id.len() == 36 && id.bytes().all(|b| b.is_ascii_hexdigit() || b == b'-'),
                "invalid save operation id"
            );
            Ok(format!("spqx:save-op:{id}"))
        })
        .transpose()?;
    ensure!(
        !name.trim().is_empty() && name.len() <= 60 && !name.chars().any(char::is_control),
        "invalid voice display name"
    );
    let (bundle, wave) = VoiceBundle::load(draft)?;
    let seconds = bundle.validate(&wave)?;
    let store = store(registry_root)?;
    fs::create_dir_all(&store.paths().root)?;
    let reference_id = format!(
        "frozen-{}",
        &hash_bytes(&serde_json::to_vec(&bundle)?)[..24]
    );
    store.edit(|registry| {
            if let Some((id,reference,audio))=operation_tag.as_deref().and_then(|tag|saved_operation(registry,tag)) {
                ensure!(safe_id(&id) && reference==reference_id && voice_id.is_none_or(|expected| expected==id),"save operation reused for another bundle or identity");
                let audio=asset_path(&store.paths().root,&audio)?;
                let (existing,_)=VoiceBundle::load(audio.parent().context("invalid saved reference path")?)?;
                ensure!(serde_json::to_vec(&existing)?==serde_json::to_vec(&bundle)?,"saved operation bundle changed");
                return Ok(id);
            }
            let id=match voice_id { Some(id)=>{ ensure!(safe_id(id) && registry.find(id).is_some(), "unknown/invalid explicit voice_id"); id.to_owned() }, None=>registry.create_identity(name) };
            ensure!(safe_id(&id), "invalid registry identity");
            let identity=registry.find_mut(&id).context("identity disappeared")?;
            if !identity.synthesis_profiles.iter().any(|p| p.engine==ENGINE) { identity.synthesis_profiles.push(SynthesisProfile::new(ENGINE)); }
            let profile=identity.synthesis_profiles.iter_mut().find(|p|p.engine==ENGINE).unwrap();
            let relative=PathBuf::from("identities").join(&id).join("synthesis").join(ENGINE).join(&reference_id);
            let destination=store.paths().root.join(&relative);
            if profile.references.iter().any(|r|r.id==reference_id) {
                let (saved,_)=VoiceBundle::load(&destination)?;
                ensure!(serde_json::to_vec(&saved)? == serde_json::to_vec(&bundle)?, "existing frozen reference changed");
                return Ok(id);
            }
            bundle.save(&destination,&wave)?;
            let audio=relative.join("reference.wav").to_string_lossy().replace('\\',"/");
            let transcript=relative.join("reference.txt").to_string_lossy().replace('\\',"/");
            profile.references.push(SynthesisReference { id:reference_id.clone(), audio:audio.clone(), transcript:transcript.clone(), reference_language:Some(bundle.language.clone()), default:profile.references.is_empty(), tags:std::iter::once(TAG.to_owned()).chain((bundle.version==2).then(||ICL_TAG.to_owned())).chain(operation_tag.clone()).collect(), description:"Explicitly selected frozen spqx audition; engine-scoped vector and PCA recipe beside reference".into(), duration_seconds:seconds, sample_rate:24000 });
            identity.sources.push(SourceRecording {id:reference_id.clone(),audio,transcript:Some(transcript),language:Some(bundle.language.clone()),note:"Saved audition archive; no recognition enrollment or training".into(),created_at_unix:std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_secs()});
            identity.touch();
            Ok(id)
    })
}

fn saved_operation(
    registry: &voqx_core::VoiceRegistry,
    tag: &str,
) -> Option<(String, String, String)> {
    registry.identities.iter().find_map(|identity| {
        identity
            .synthesis_profile(ENGINE)?
            .references
            .iter()
            .find(|reference| reference.tags.iter().any(|value| value == tag))
            .map(|reference| {
                (
                    identity.id.clone(),
                    reference.id.clone(),
                    reference.audio.clone(),
                )
            })
    })
}

pub fn resolve(
    voice_id: &str,
    reference_id: Option<&str>,
    registry_root: Option<&Path>,
) -> Result<crate::engine::Reference> {
    ensure!(safe_id(voice_id), "invalid canonical voice_id");
    let store = store(registry_root)?;
    let registry = store.load()?;
    let identity = registry
        .find(voice_id)
        .context("canonical voice_id not found")?;
    let profile = identity
        .synthesis_profile(ENGINE)
        .context("identity has no spqx synthesis profile")?;
    let reference = if let Some(id) = reference_id {
        profile.references.iter().find(|r| r.id == id)
    } else {
        let defaults: Vec<_> = profile.references.iter().filter(|r| r.default).collect();
        if defaults.len() == 1 {
            Some(defaults[0])
        } else if profile.references.len() == 1 {
            profile.references.first()
        } else {
            None
        }
    }
    .context("select an explicit reference_id; no unique default reference")?;
    let wav = asset_path(&store.paths().root, &reference.audio)?;
    let transcript = asset_path(&store.paths().root, &reference.transcript)?;
    let text = fs::read_to_string(transcript)?;
    let (frozen, seed, icl_context) = if reference
        .tags
        .iter()
        .any(|tag| tag == TAG || tag == ICL_TAG)
    {
        let (bundle, _) = VoiceBundle::load(wav.parent().context("reference has no parent")?)?;
        ensure!(text == bundle.text, "canonical transcript changed");
        (Some(bundle.conditioning), bundle.seed, bundle.icl_context)
    } else {
        (None, None, None)
    };
    Ok(crate::engine::Reference {
        wav,
        text,
        label: voice_id.into(),
        frozen,
        icl_context,
        seed,
    })
}
