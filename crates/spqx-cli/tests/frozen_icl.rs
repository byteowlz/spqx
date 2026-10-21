#![cfg(feature = "qwentts")]
//! Explicitly admitted native proof, never part of model-free CI inference.
use spqx_core::{
    audio::write_wav_float_bytes,
    conditioning::hash_bytes,
    icl::{FrozenIcl, IclContext},
    synthesis::{Backend, EngineOptions, Sampling, Session, Stop},
};
use std::{
    env, fs,
    path::{Path, PathBuf},
    process::Command,
    time::Instant,
};

const TEXT:&str="Hallo, schön, dass du da bist. Für zwölf Euro gibt es frischen Kaffee und Käsekuchen. Morgen fahren wir nach München.";
fn required(name: &str) -> PathBuf {
    PathBuf::from(
        env::var_os(name)
            .unwrap_or_else(|| panic!("set {name} explicitly; no downloads/admission fallback")),
    )
}
fn sampling() -> Sampling {
    Sampling {
        temperature: 0.9,
        top_k: 1,
        max_new_tokens: 256,
        chunk_size: 4,
    }
}
fn render(session: &Session, text: &str) -> Vec<f32> {
    let mut samples = Vec::new();
    let stop = session
        .synthesize(
            text,
            &sampling(),
            |part, rate| {
                assert_eq!(rate, 24000);
                samples.extend_from_slice(part);
                true
            },
            || false,
        )
        .unwrap();
    assert_eq!(stop, Stop::Eos);
    assert!(samples.len() > 24000 && samples.len() < 480000);
    assert!(samples.iter().all(|x| x.is_finite() && x.abs() < 1.));
    assert!(
        samples.iter().map(|x| (*x as f64).powi(2)).sum::<f64>() / samples.len() as f64 > 1e-10
    );
    samples
}
fn write_wave(root: &Path, name: &str, samples: &[f32]) {
    fs::write(
        root.join(name),
        write_wav_float_bytes(samples, 24000).unwrap(),
    )
    .unwrap();
}
fn interruptions(session: &Session) {
    let mut called = 0;
    assert_eq!(
        session
            .synthesize(
                TEXT,
                &sampling(),
                |_, _| {
                    called += 1;
                    true
                },
                || true
            )
            .unwrap(),
        Stop::Cancelled
    );
    assert_eq!(called, 0);
    assert_eq!(
        session
            .synthesize(
                TEXT,
                &sampling(),
                |_, _| {
                    called += 1;
                    false
                },
                || false
            )
            .unwrap(),
        Stop::Cancelled
    );
    assert_eq!(called, 1);
    let mut cap = sampling();
    cap.max_new_tokens = 2;
    assert_eq!(
        session
            .synthesize(TEXT, &cap, |_, _| true, || false)
            .unwrap(),
        Stop::CapReached
    );
    assert!(session
        .synthesize("invalid\0text", &sampling(), |_, _| true, || false)
        .is_err());
}
fn reject_wrong_width(prompt: &FrozenIcl, model: &Path, options: &EngineOptions) {
    let mut speaker = prompt.speaker.clone();
    speaker.values.push(0.);
    speaker.scope.dimension += 1;
    let context = IclContext::from_native(
        speaker.scope.clone(),
        prompt.context.frames,
        prompt.context.codes.clone(),
        prompt.context.text.clone(),
        prompt.context.source_audio_sha256.clone(),
    )
    .unwrap();
    let bad = FrozenIcl::new(speaker, context).unwrap();
    if let Ok(session) = Session::load_frozen_icl(model, &bad, "german", Some(4242), options) {
        let mut called = false;
        assert!(session
            .synthesize(
                TEXT,
                &sampling(),
                |_, _| {
                    called = true;
                    true
                },
                || false
            )
            .is_err());
        assert!(!called);
    }
}

#[test]
#[ignore = "requires explicit pinned assets/runtime, lawful foreign-language reference and fresh bounded admission"]
fn native_frozen_icl_contract() {
    let model = required("SPQX_ICL_MODEL");
    let codec = required("SPQX_ICL_CODEC");
    let library = required("SPQX_ICL_LIBRARY");
    let reference = required("SPQX_ICL_REFERENCE");
    let transcript = fs::read_to_string(required("SPQX_ICL_TRANSCRIPT")).unwrap();
    let output = required("SPQX_ICL_OUTPUT");
    fs::create_dir(&output).expect("fresh owned output directory required");
    let device = env::var("SPQX_ICL_DEVICE").expect("set explicit device; no auto substitution");
    let options = EngineOptions {
        backend: Backend::Ggml,
        device: device.clone(),
        codec: Some(codec.clone()),
        library: Some(library.clone()),
    };
    let input = output.join("original-input.wav");
    fs::copy(reference, &input).unwrap();
    let start = Instant::now();
    // Exactly one loaded model at a time, including the deliberate dimension failure.
    let (prompt, original) = {
        let session = Session::load(&model, &input, &transcript, "german", &options).unwrap();
        let prompt = session.frozen_icl().unwrap();
        let original = render(&session, TEXT);
        (prompt, original)
    };
    write_wave(&output, "original-reference.wav", &original);
    let document = serde_json::to_vec_pretty(&prompt).unwrap();
    fs::write(output.join("prompt.json"), &document).unwrap();
    fs::rename(&input, output.join("archived-source.wav")).unwrap();
    assert!(!input.exists());
    let loaded: FrozenIcl =
        serde_json::from_slice(&fs::read(output.join("prompt.json")).unwrap()).unwrap();
    loaded.validate().unwrap();
    assert_eq!(loaded.context.codes, prompt.context.codes);
    assert_eq!(loaded.context.sha256, prompt.context.sha256);
    assert_eq!(
        loaded
            .speaker
            .values
            .iter()
            .map(|x| x.to_bits())
            .collect::<Vec<_>>(),
        prompt
            .speaker
            .values
            .iter()
            .map(|x| x.to_bits())
            .collect::<Vec<_>>()
    );
    reject_wrong_width(&loaded, &model, &options);
    let (frozen, recovery) = {
        let session =
            Session::load_frozen_icl(&model, &loaded, "german", Some(4242), &options).unwrap();
        let frozen = render(&session, TEXT);
        assert_eq!(
            session.frozen_icl().unwrap().context.sha256,
            prompt.context.sha256
        );
        assert!(
            session.frozen_conditioning().is_err(),
            "no implicit vector-only downgrade"
        );
        interruptions(&session);
        let recovery = render(
            &session,
            "Heute prüfen wir die gespeicherte Stimme ohne die ursprüngliche Aufnahme.",
        );
        (frozen, recovery)
    };
    write_wave(&output, "frozen-icl.wav", &frozen);
    write_wave(&output, "recovery.wav", &recovery);
    let config = output.join("native.toml");
    fs::write(&config,format!("[model]\npath={:?}\n[engine]\nbackend=\"ggml\"\ndevice={:?}\nlibrary={:?}\ncodec={:?}\n[sampling]\ntemperature=0.9\ntop_k=1\n",model.to_str().unwrap(),device,library.to_str().unwrap(),codec.to_str().unwrap())).unwrap();
    let cli = Command::new(env!("CARGO_BIN_EXE_spqx"))
        .args([
            "--config",
            config.to_str().unwrap(),
            "--json",
            "lab",
            "preview",
            TEXT,
            "--icl",
            output.join("prompt.json").to_str().unwrap(),
            "--seed",
            "4242",
            "--out",
            output.join("fresh-cli").to_str().unwrap(),
        ])
        .env("XDG_CONFIG_HOME", output.join("private-config"))
        .env("XDG_DATA_HOME", output.join("private-data"))
        .output()
        .unwrap();
    fs::write(output.join("cli.stderr.log"), &cli.stderr).unwrap();
    assert!(
        cli.status.success(),
        "{}",
        String::from_utf8_lossy(&cli.stderr)
    );
    let bundle: serde_json::Value =
        serde_json::from_slice(&fs::read(output.join("fresh-cli/voice.json")).unwrap()).unwrap();
    assert_eq!(bundle["version"], 2);
    assert_eq!(bundle["icl_context"]["sha256"], loaded.context.sha256);
    let legacy_rejected = env::var_os("SPQX_ICL_OLD_CLI").map(|old| {
        let result = Command::new(old)
            .args(["lab", "inspect", output.join("fresh-cli").to_str().unwrap()])
            .env("XDG_CONFIG_HOME", output.join("private-config"))
            .output()
            .unwrap();
        fs::write(output.join("legacy.stderr.log"), &result.stderr).unwrap();
        assert!(!result.status.success());
        assert!(String::from_utf8_lossy(&result.stderr).contains("unknown field"));
        true
    });
    let a = write_wav_float_bytes(&original, 24000).unwrap();
    let b = write_wav_float_bytes(&frozen, 24000).unwrap();
    let c = fs::read(output.join("fresh-cli/reference.wav")).unwrap();
    let report = serde_json::json!({"elapsed_seconds":start.elapsed().as_secs_f64(),"original_sha256":hash_bytes(&a),"frozen_sha256":hash_bytes(&b),"fresh_cli_sha256":hash_bytes(&c),"exact_original_frozen_pcm":a==b,"exact_api_cli_pcm":b==c,"scope":loaded.speaker.scope,"context_sha256":loaded.context.sha256,"codebooks":loaded.context.codebooks,"frames":loaded.context.frames,"original_reference_path_absent":!input.exists(),"wrong_width_rejected_before_pcm":true,"cancel_cap_malformed_recovery":true,"legacy_reader_rejected_v2":legacy_rejected,"human_accent_approval":false,"canonical_registration":false});
    fs::write(
        output.join("verification.json"),
        serde_json::to_vec_pretty(&report).unwrap(),
    )
    .unwrap();
    println!("{report}");
}
