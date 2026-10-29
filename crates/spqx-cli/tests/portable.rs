//! Model-free CLI and binary-stdout regressions, including Windows CRT paths.
use std::{
    path::PathBuf,
    process::{Command, Stdio},
    sync::atomic::{AtomicUsize, Ordering},
};
static NEXT: AtomicUsize = AtomicUsize::new(0);
struct Scratch(PathBuf);
impl Scratch {
    fn new() -> Self {
        let p = std::env::temp_dir().join(format!(
            "spqx-portable-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&p).unwrap();
        Self(p)
    }
    fn command(&self) -> Command {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_spqx"));
        for key in [
            "XDG_CONFIG_HOME",
            "XDG_DATA_HOME",
            "XDG_STATE_HOME",
            "XDG_CACHE_HOME",
            "APPDATA",
            "LOCALAPPDATA",
        ] {
            cmd.env(key, &self.0);
        }
        for key in [
            "SPQX_OFFLINE",
            "HF_HUB_OFFLINE",
            "SPQX_CONFIG",
            "SPQX_BACKEND",
            "SPQX_DEVICE",
            "SPQX_CODEC",
            "SPQX_ENGINE_LIBRARY",
        ] {
            cmd.env_remove(key);
        }
        cmd
    }
    fn config(&self, args: &[&str]) -> serde_json::Value {
        let output = self
            .command()
            .args(args)
            .args(["--json", "config", "show"])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice(&output.stdout).unwrap()
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
#[test]
fn engine_flags_override_config_and_json_stays_clean() {
    let scratch = Scratch::new();
    let value = scratch.config(&["--backend", "ggml", "--device", "cpu", "--offline"]);
    assert_eq!(value["engine"]["backend"], "ggml");
    assert_eq!(value["engine"]["device"], "cpu");
    assert_eq!(value["model"]["offline"], true);
    assert_eq!(value["model"]["auto_download"], true);
}
#[test]
fn design_and_model_download_commands_coexist_without_base_acquisition() {
    let scratch = Scratch::new();
    for args in [
        vec!["design", "--help"],
        vec!["models", "download", "--help"],
    ] {
        let output = scratch.command().args(args).output().unwrap();
        assert!(output.status.success(), "{:?}", output);
    }
    let out = scratch.0.join("candidate.wav");
    let cache = scratch.0.join("empty-cache");
    let output = scratch
        .command()
        .env("SPQX_MODELS_DIR", &cache)
        .args([
            "--offline",
            "--backend",
            "ggml",
            "--json",
            "design",
            "Synthetic text.",
            "--instruct",
            "A fictional adult German voice",
            "--no-play",
            "--out",
        ])
        .arg(&out)
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(output.stdout.is_empty(), "{:?}", output);
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("qwen-talker-1.7b-voicedesign-Q8_0.gguf"),
        "{:?}",
        output
    );
    assert!(!cache.exists());
    assert!(!out.exists());
}

#[test]
fn design_headless_requires_an_output_before_model_acquisition() {
    let scratch = Scratch::new();
    let output = scratch
        .command()
        .args([
            "design",
            "Synthetic text.",
            "--instruct",
            "A fictional voice",
            "--no-play",
        ])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("specify --out"));
}

#[cfg(feature = "qwentts")]
#[test]
fn startup_error_is_one_binary_frame_not_engine_chatter() {
    let scratch = Scratch::new();
    let model = scratch.0.join("model");
    std::fs::create_dir(&model).unwrap();
    std::fs::write(model.join("qwen-talker-0.6b-base-Q8_0.gguf"), []).unwrap();
    std::fs::write(model.join("qwen-tokenizer-12hz-F32.gguf"), []).unwrap();
    let wav = scratch.0.join("synthetic.wav");
    spqx_core::audio::write_wav_file(wav.to_str().unwrap(), &[0.0; 240], 24000).unwrap();
    let value = serde_json::json!({"model":{"path":model},"engine":{"backend":"ggml","device":"cpu","library":scratch.0.join("missing-native-runtime")}});
    let config = scratch.0.join("config.toml");
    std::fs::write(&config, toml::to_string(&value).unwrap()).unwrap();
    let output = scratch
        .command()
        .arg("--config")
        .arg(config)
        .args(["serve", "--ref-audio"])
        .arg(wav)
        .args(["--ref-text", "Synthetic fixture."])
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(output.stdout.len() > 9, "{:?}", output);
    let bytes = &output.stdout;
    assert_eq!(bytes[0], 5);
    assert_eq!(&bytes[1..5], &[0; 4]);
    let len = u32::from_le_bytes(bytes[5..9].try_into().unwrap()) as usize;
    assert_eq!(bytes.len(), 9 + len);
    assert!(std::str::from_utf8(&bytes[9..])
        .unwrap()
        .contains("loading"));
}
