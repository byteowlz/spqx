use super::*;
use spqx_core::{
    audio::write_wav_float_bytes,
    conditioning::{ConditioningBackend, ConditioningScope, PcaSpace},
};

pub(super) fn fixture() -> (VoiceBundle, Vec<u8>) {
    let scope = ConditioningScope {
        backend: ConditioningBackend::Ggml,
        model_sha256: "a".repeat(64),
        codec_sha256: "b".repeat(64),
        encoder: "unit-test-only".into(),
        dimension: 3,
        representation: "raw-f32".into(),
    };
    let pca = PcaSpace::fit(
        scope,
        [
            ("A".into(), vec![2., 0., 7.]),
            ("C".into(), vec![0., 2., 7.]),
            ("D".into(), vec![-2., 0., 7.]),
        ]
        .into(),
        2,
        0.75,
    )
    .unwrap();
    let conditioning = pca.edit("A", &[0.3, -0.2]).unwrap();
    let samples: Vec<f32> = (0..48000).map(|i| (i as f32 * 0.07).sin() * 0.1).collect();
    let wave = write_wav_float_bytes(&samples, 24000).unwrap();
    (
        VoiceBundle {
            version: 1,
            conditioning,
            icl_context: None,
            pca: Some(pca),
            text: "Synthetic fixture only.".into(),
            language: "german".into(),
            seed: Some(4242),
            reference_sha256: hash_bytes(&wave),
            provenance: BTreeMap::new(),
        },
        wave,
    )
}
#[test]
fn exact_wave_vector_and_editor_bank_survive_reopen() {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join("draft");
    let (voice, wave) = fixture();
    voice.save(&dir, &wave).unwrap();
    let (loaded, bytes) = VoiceBundle::load(&dir).unwrap();
    assert_eq!(bytes, wave);
    assert_eq!(loaded.conditioning.values, voice.conditioning.values);
    assert_eq!(loaded.conditioning.scope, voice.conditioning.scope);
    assert_eq!(
        loaded.pca.as_ref().unwrap().id().unwrap(),
        voice.pca.as_ref().unwrap().id().unwrap()
    );
    assert!(voice.save(&dir, &wave).is_err());
}
#[test]
fn tampered_wave_text_and_recipe_are_not_saved_as_that_audition() {
    let temp = tempfile::tempdir().unwrap();
    let (mut voice, wave) = fixture();
    voice.conditioning.recipe.as_mut().unwrap().offsets[0] = 0.4;
    assert!(voice
        .save(&temp.path().join("wrong-settings"), &wave)
        .is_err());
    let (voice, wave) = fixture();
    let dir = temp.path().join("draft");
    voice.save(&dir, &wave).unwrap();
    fs::write(dir.join("reference.txt"), "Different words").unwrap();
    assert!(VoiceBundle::load(&dir).is_err());
    fs::write(dir.join("reference.txt"), &voice.text).unwrap();
    let mut changed = wave.clone();
    let i = changed.len() - 1;
    changed[i] ^= 1;
    fs::write(dir.join("reference.wav"), changed).unwrap();
    assert!(VoiceBundle::load(&dir).is_err());
}
#[test]
fn canonical_promotion_is_explicit_synthesis_only_and_id_survives_rename() {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join("draft");
    let registry = temp.path().join("registry");
    let (voice, wave) = fixture();
    voice.save(&dir, &wave).unwrap();
    let id = promote(&dir, "Fixture voice", None, Some(&registry)).unwrap();
    let saved = store(Some(&registry)).unwrap().load().unwrap();
    let identity = saved.find(&id).unwrap();
    assert!(identity.recognition_profiles.is_empty());
    assert!(identity.wake_profiles.is_empty());
    assert_eq!(identity.sources.len(), 1);
    assert_eq!(
        identity.synthesis_profile(ENGINE).unwrap().references.len(),
        1
    );
    assert_eq!(
        promote(&dir, "Unused new name", Some(&id), Some(&registry)).unwrap(),
        id
    );
    store(Some(&registry))
        .unwrap()
        .edit(|r| r.rename(&id, "Renamed fixture"))
        .unwrap();
    let selected = resolve(&id, None, Some(&registry)).unwrap();
    assert_eq!(selected.frozen.unwrap().values, voice.conditioning.values);
    assert_eq!(selected.seed, Some(4242));
    let other = promote(&dir, "Fixture voice", None, Some(&registry)).unwrap();
    assert_ne!(other, id);
    assert!(resolve("../../escape", None, Some(&registry)).is_err());
    let selected = resolve(&id, None, Some(&registry)).unwrap();
    fs::remove_file(selected.wav.parent().unwrap().join("voice.json")).unwrap();
    assert!(resolve(&id, None, Some(&registry)).is_err()); // no reference re-extraction fallback
}
#[test]
fn retry_key_is_durable_without_merging_distinct_save_intents() {
    let temp = tempfile::tempdir().unwrap();
    let registry = temp.path().join("registry");
    let draft = temp.path().join("draft");
    let (voice, wave) = fixture();
    voice.save(&draft, &wave).unwrap();
    let key = "3f5d425e-9e98-46c7-8d4c-6da715bf53ef";
    let first = promote_once(&draft, "Fixture", None, Some(&registry), Some(key)).unwrap();
    let second = promote_once(&draft, "Fixture", None, Some(&registry), Some(key)).unwrap();
    assert_eq!(first, second);
    assert_eq!(
        store(Some(&registry))
            .unwrap()
            .load()
            .unwrap()
            .identities
            .len(),
        1
    );
    let mut changed = voice.clone();
    changed
        .provenance
        .insert("note".into(), "another save intent".into());
    let other = temp.path().join("other");
    changed.save(&other, &wave).unwrap();
    assert!(promote_once(&other, "Fixture", None, Some(&registry), Some(key)).is_err());
}
#[cfg(unix)]
#[test]
fn symlink_bundle_asset_is_rejected() {
    let temp = tempfile::tempdir().unwrap();
    let (voice, wave) = fixture();
    let dir = temp.path().join("draft");
    voice.save(&dir, &wave).unwrap();
    fs::rename(dir.join("reference.wav"), temp.path().join("elsewhere.wav")).unwrap();
    std::os::unix::fs::symlink(temp.path().join("elsewhere.wav"), dir.join("reference.wav"))
        .unwrap();
    assert!(VoiceBundle::load(&dir).is_err());
}
