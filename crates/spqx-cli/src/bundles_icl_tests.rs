use super::*;
use spqx_core::icl::{IclContext, QWEN_12HZ_CODEC};

fn full_fixture() -> (VoiceBundle, Vec<u8>) {
    let (mut voice, wave) = super::tests::fixture();
    voice.version = 2;
    voice.conditioning.scope.codec_sha256 = QWEN_12HZ_CODEC.into();
    let pca = voice.pca.as_mut().unwrap();
    pca.scope = voice.conditioning.scope.clone();
    voice.conditioning.recipe.as_mut().unwrap().space = pca.id().unwrap();
    voice.icl_context = Some(
        IclContext::from_native(
            voice.conditioning.scope.clone(),
            2,
            (0..32).collect(),
            "Original English reference, not this audition.".into(),
            "d".repeat(64),
        )
        .unwrap(),
    );
    voice.text = "Neuer deutscher Satz.".into();
    (voice, wave)
}
#[test]
fn vector_v1_has_no_new_wire_fields_and_keeps_legacy_mode() {
    let (voice, wave) = super::tests::fixture();
    let bytes = serde_json::to_vec(&voice).unwrap();
    let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert!(json.get("icl_context").is_none());
    assert_eq!(json["version"], 1);
    let reloaded: VoiceBundle = serde_json::from_slice(&bytes).unwrap();
    reloaded.validate(&wave).unwrap();
    assert_eq!(serde_json::to_vec(&reloaded).unwrap(), bytes);
}
#[test]
fn full_icl_bundle_reopens_original_context_and_canonical_consumer() {
    let (voice, wave) = full_fixture();
    let temp = tempfile::tempdir().unwrap();
    let draft = temp.path().join("draft");
    let registry = temp.path().join("registry");
    voice.save(&draft, &wave).unwrap();
    let (saved, _) = VoiceBundle::load(&draft).unwrap();
    assert_eq!(
        saved.icl_context.as_ref().unwrap().sha256,
        voice.icl_context.as_ref().unwrap().sha256
    );
    let id = promote(&draft, "Synthetic ICL fixture", None, Some(&registry)).unwrap();
    let reference = resolve(&id, None, Some(&registry)).unwrap();
    assert_eq!(reference.text, voice.text);
    assert_ne!(reference.text, reference.icl_context.as_ref().unwrap().text);
    assert_eq!(
        reference.icl_context.unwrap().codes,
        voice.icl_context.unwrap().codes
    );
    let data = store(Some(&registry)).unwrap().load().unwrap();
    let profile = data.find(&id).unwrap().synthesis_profile(ENGINE).unwrap();
    assert!(profile.references[0].tags.iter().any(|x| x == TAG));
    assert!(profile.references[0].tags.iter().any(|x| x == ICL_TAG));
    assert!(data.find(&id).unwrap().recognition_profiles.is_empty());
}
#[test]
fn modes_and_context_tampering_fail_without_downgrade() {
    let (mut voice, wave) = full_fixture();
    voice.version = 1;
    assert!(voice.validate(&wave).is_err());
    voice.version = 2;
    voice.icl_context.as_mut().unwrap().text.push('!');
    assert!(voice.validate(&wave).is_err());
    voice.icl_context = None;
    assert!(voice.validate(&wave).is_err());
}
