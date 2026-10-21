# Frozen-voice qualification

Scope: feature branch implementation, not a merged/default release, platform parity or human acoustic approval.

## Model-free

- Portable `cargo check` and34 tests passed;3 model-dependent tests remain opt-in/ignored.
- Apple Silicon `cargo check --no-default-features --features mlx,qwentts` passed with scoped `MLX_METAL_JIT` CMake configuration, without a global Metal SDK installation. This is compilation, not MLX frozen-input/VoiceDesign inference.
- Voice Lab18 tests/85 assertions and Bun server build passed.
- Tests exercise centered-SVD neutral/residual identity, finite/rank/scope failures, vector/bank/WAV reload and tamper/symlink rejection, canonical rename/reopen, explicit synthesis-only saves and durable retry keys.

## Actual native execution

Pinned qwentts.cpp `5916bf7c79dbb4e66fc059cdc507a79a23a28bb4` / GGML `cc5df49a6b878db2b68b9a7b45976e279594627d`, ABI6. Qwen0.6B Base Q8 model SHA256 `d54dbaf10591421fa764ed630d764efa717ae40cd959bd48c66d4eb1af226426`; codec `b16b95557c7c7340a121757bd6855b9609e1cf4ad3fad0778b89393293ae5f3d`. Kept fictional source auditions A/C/D were encoded by this native model; B excluded. No SDK vector was recast into native scope.

An admitted5GiB/300s CUDA run fitted two directions, rendered baseline/axis extremes/arbitrary `[0.3,-0.2]`, retained exact vector/WAV/bank, explicitly promoted in an isolated voqx registry and synthesized new text after canonical reload. Wrong scope/dimension failed. Approx34s bounded proof, sampled owned peak3290MiB; lease released. Sampling is not a continuous peak guarantee.

The actual native Web/CLI path then passed with a fresh isolated loopback service/registry and a separate4GiB/240s lease:

- Keyboard PCA edits cause no render; explicit Preview captured `[0.3,-0.2]` while a later input changed to0.35. Completed audio/settings were not relabelled.
- Browser decoded native float WAV and its playback clock advanced. A/B pin, canonical Save, retry, reload, Range206 and missing-Origin rejection passed without credentials.
- Preview WAV bytes and complete native JSON bytes were identical across job, draft and canonical assets. Exact fitted bank/recipe and effective vector survived.
- Simulated response loss after the registry committed, but before app acknowledgement, returned the same canonical id on retry without additional generation.
- One observed click-to-ready time3.097s; native process preparation1.546s, first PCM0.124s after preparation, generation0.740s, total2.319s. Provider wall2.722s includes monitoring/IO. These are single observations, not percentiles or physical playback. Sampled owned peak2778MiB; reservation released; no idle model retained.

Global registry was not used for automated proof. No recognition/wake profile, weight training, real-person clone, cloud fallback or shared workload stop occurred. Private artifacts/receipts remain outside git. No new native PCA ASR or human usefulness/accent/character approval is claimed.

## Review limits

Scoped quality-delta has four narrow acknowledgements: intentional tiny typed-constructor wrapper similarity and self short-horizon churn on the existing engine seam. Say-command complexity and execution-dispatch duplication were reduced. Remaining new-symbol/prototype controller findings are visible, not erased by exit0. Static call-graph coverage is not complete; test execution and runtime evidence above are the positive proof, not blanket merge approval.

Desktop1280px and phone390px captures use a read-only synthetic fixture; existing r2 look/tokens retained, no horizontal overflow. Browser phone viewport is not a physical phone. No selected Studio production target or independent finish/acoustic reviewer is implied. Controller/quota/shutdown hardening, broader bank calibration, persistent warm editing and MLX inference remain separate work.
