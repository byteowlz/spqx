//! Synthetic loopback fixtures only; no public model downloads in unit tests.
use super::*;
use std::{net::TcpListener, sync::Arc, thread};

fn profile(body: &[u8]) -> Profile {
    Profile {
        purpose: Purpose::Clone,
        backend: spqx_core::synthesis::Backend::Ggml,
        entrypoint: None,
        repo: "test/model".into(),
        revision: "1111111111111111111111111111111111111111".into(),
        files: vec![Asset {
            path: "nested/weights.bin".into(),
            bytes: body.len() as u64,
            sha256: format!("{:x}", Sha256::digest(body)),
        }],
    }
}
fn server(body: &[u8], declared: usize) -> (String, thread::JoinHandle<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let address = listener.local_addr().unwrap();
    let body = body.to_vec();
    let join = thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut stream = loop {
            match listener.accept() {
                Ok((s, _)) => break s,
                Err(_) if Instant::now() < deadline => thread::sleep(Duration::from_millis(5)),
                Err(e) => panic!("fixture did not receive request: {e}"),
            }
        };
        stream
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        let mut request = [0u8; 4096];
        let n = stream.read(&mut request).unwrap();
        write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Length: {declared}\r\nConnection: close\r\n\r\n"
        )
        .unwrap();
        stream.write_all(&body).unwrap();
        String::from_utf8_lossy(&request[..n]).into_owned()
    });
    (format!("http://{address}"), join)
}
fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout_read(Duration::from_secs(3))
        .build()
}

#[test]
fn pinned_download_then_offline_reuse_and_corruption_detection() {
    let cache = tempfile::tempdir().unwrap();
    let body = b"synthetic model";
    let p = profile(body);
    let (origin, server) = server(body, body.len());
    let path = acquire(&p, cache.path(), false, &agent(), &origin).unwrap();
    assert!(server.join().unwrap().starts_with(
        "GET /test/model/resolve/1111111111111111111111111111111111111111/nested/weights.bin "
    ));
    assert_eq!(
        acquire(&p, cache.path(), true, &agent(), "http://127.0.0.1:1").unwrap(),
        path
    );
    fs::write(path.join("nested/weights.bin"), b"corrupt").unwrap();
    assert!(
        acquire(&p, cache.path(), true, &agent(), "http://127.0.0.1:1")
            .unwrap_err()
            .to_string()
            .contains("offline")
    );
}
#[test]
fn bad_hash_and_truncated_transfer_never_commit_a_model() {
    for (body, declared) in [
        (b"tampered payload".as_slice(), 16),
        (b"short".as_slice(), 16),
    ] {
        let cache = tempfile::tempdir().unwrap();
        let p = profile(b"expected payload");
        let (origin, server) = server(body, declared);
        assert!(acquire(&p, cache.path(), false, &agent(), &origin).is_err());
        server.join().unwrap();
        let path = cache
            .path()
            .join("models--test--model/snapshots")
            .join(&p.revision)
            .join("nested/weights.bin");
        assert!(!path.exists());
    }
}
#[test]
fn concurrent_consumers_share_one_atomic_download() {
    let cache = tempfile::tempdir().unwrap();
    let p = Arc::new(profile(b"one transfer"));
    let (origin, server) = server(b"one transfer", 12);
    thread::scope(|scope| {
        let a = scope.spawn(|| acquire(&p, cache.path(), false, &agent(), &origin));
        let b = scope.spawn(|| acquire(&p, cache.path(), false, &agent(), &origin));
        assert_eq!(a.join().unwrap().unwrap(), b.join().unwrap().unwrap());
    });
    server.join().unwrap();
}
#[test]
fn offline_missing_cache_does_not_create_state_or_contact_server() {
    let temp = tempfile::tempdir().unwrap();
    let cache = temp.path().join("absent");
    assert!(acquire(
        &profile(b"offline"),
        &cache,
        true,
        &agent(),
        "http://127.0.0.1:1"
    )
    .is_err());
    assert!(!cache.exists());
}
#[test]
fn catalog_is_pinned_and_paths_are_confined() {
    let catalog = profiles().unwrap();
    assert_eq!(catalog.len(), 4);
    use spqx_core::synthesis::Backend;
    let base_id = &catalog[0].repo;
    let design = select_profile(&catalog, base_id, Backend::Ggml, Purpose::Design)
        .unwrap()
        .unwrap();
    assert!(design
        .files
        .iter()
        .any(|asset| asset.path == "qwen-talker-1.7b-voicedesign-Q8_0.gguf"));
    assert!(!design
        .files
        .iter()
        .any(|asset| asset.path.contains("base-")));
    assert_eq!(
        select_profile(&catalog, base_id, Backend::Ggml, Purpose::Clone)
            .unwrap()
            .unwrap()
            .purpose,
        Purpose::Clone
    );
    let mlx = select_profile(&catalog, base_id, Backend::Mlx, Purpose::Design)
        .unwrap()
        .unwrap();
    assert!(mlx.repo.contains("VoiceDesign") && mlx.entrypoint.is_none());
    assert!(mlx
        .files
        .iter()
        .any(|asset| asset.path == "speech_tokenizer/model.safetensors"));
    // Auto follows the configured built-in profile's engine; Base remains the clone default.
    let auto = select_profile(&catalog, base_id, Backend::Auto, Purpose::Design)
        .unwrap()
        .unwrap();
    assert_eq!(auto.backend, catalog[0].backend);
    assert!(select_profile(&catalog, base_id, Backend::Tch, Purpose::Design).is_err());
    assert!(
        select_profile(&catalog, "custom/model", Backend::Ggml, Purpose::Design)
            .unwrap()
            .is_some_and(
                |profile| profile.purpose == Purpose::Design && profile.backend == Backend::Ggml
            )
    );
    for p in catalog {
        assert_eq!(p.revision.len(), 40);
        for asset in p.files {
            assert_eq!(asset.sha256.len(), 64);
            assert!(asset.bytes > 0);
            asset_path(Path::new("cache"), &asset).unwrap();
        }
    }
    for name in ["../escape", "/escape", "C:\\escape", ""] {
        let a = Asset {
            path: name.into(),
            bytes: 1,
            sha256: String::new(),
        };
        assert!(asset_path(Path::new("cache"), &a).is_err());
    }
}
#[test]
fn cloning_model_settings_never_select_voice_design_weights() {
    use spqx_core::synthesis::Backend;
    let catalog = profiles().unwrap();
    // A custom cloning model ID stays cache-only for cloning, but design still
    // gets the pinned VoiceDesign profile for this build's default engine.
    let custom = "mlx-community/Qwen3-TTS-12Hz-0.6B-Base-4bit";
    assert!(
        select_profile(&catalog, custom, Backend::Auto, Purpose::Clone)
            .unwrap()
            .is_none()
    );
    let design = select_profile(&catalog, custom, Backend::Auto, Purpose::Design)
        .unwrap()
        .unwrap();
    assert_eq!(
        (design.purpose, design.backend),
        (Purpose::Design, default_backend())
    );
    let temp = tempfile::tempdir().unwrap();
    let mut config = Config::default();
    config.model.path = Some(temp.path().join("base-only").display().to_string());
    config.model.design_path = Some(temp.path().display().to_string());
    assert_eq!(resolve_for(&config, Purpose::Design).unwrap(), temp.path());
    assert!(resolve_for(&config, Purpose::Clone).is_err());
    config.model.design_path = Some(temp.path().join("missing").display().to_string());
    assert!(resolve_for(&config, Purpose::Design)
        .unwrap_err()
        .to_string()
        .contains("model.design_path"));
}
#[test]
fn explicit_path_is_local_and_never_replaced() {
    let temp = tempfile::tempdir().unwrap();
    let mut config = Config::default();
    config.model.path = Some(temp.path().display().to_string());
    assert_eq!(resolve(&config).unwrap(), temp.path());
    config.model.path = Some(temp.path().join("missing").display().to_string());
    assert!(resolve(&config)
        .unwrap_err()
        .to_string()
        .contains("explicit paths"));
}
