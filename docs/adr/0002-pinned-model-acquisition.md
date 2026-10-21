# ADR 0002: Automatic pinned model acquisition

Status: accepted.

## Decision

The user-facing CLI acquires missing model files on first `say`/`serve` use. `spqx models download` explicitly prefetches without synthesis or a reference voice. Default profiles are the tested MLX 0.6B Base 6-bit snapshot and GGML 0.6B Base Q8 plus F32 codec. The built-in catalog pins revisions, exact file sizes and SHA256, including MLX configuration/tokenizer assets; it never snapshots the entire multi-variant GGUF repository.

This permits model-file downloads, not hosted inference, reference-audio downloads, enrollment or uploads. Runtime bundles still exclude weights and recordings. The low-level config-free `spqx-tts-worker` retains explicit local model paths; controllers can prefetch using the CLI.

Explicit local paths are never repaired or replaced. Custom model IDs remain cache-only until a reviewed download profile exists. The default profile follows available platform features; an explicitly selected engine picks its compatible built-in format. A missing MLX backend fails before downloading its weights.

`--offline` / `SPQX_OFFLINE`, `[model] offline=true` and `HF_HUB_OFFLINE=1` prohibit model requests; `model.auto_download=false` disables implicit acquisition (explicit `models download` can prefetch unless offline). Cache selection is SPQX_MODELS_DIR, HF_HUB_CACHE, HF_HOME/hub, paths.models_dir, then the existing user HF cache.

A per-repository OS file lock serializes consumers. Files stream into same-directory temporary files, are size/hash checked, fsynced and atomically committed. Interrupted/corrupt transfers cannot become usable model files; subsequent attempts retain valid completed files. An intact cached snapshot works offline. Existing supported snapshots are integrity-checked before use, so cache verification is startup work, not streaming TTFA.

## Proof

Synthetic loopback tests cover exact pinned URLs, cache-only reuse, corruption, truncated/wrong-content responses, concurrent consumers, confined paths, explicit-path preservation and zero-network offline failure. Live prefetch and native synthesis are separate opt-in checks. CLI JSON and binary stdout remain reserved for results/protocol; download progress goes to stderr.
