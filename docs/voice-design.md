# Native VoiceDesign

`spqx design` generates a new described voice, without a reference recording or a preset speaker. It runs on the existing engines: the whole-pipeline GGML/libqwen runtime and native MLX, never a Python SDK adapter. The model stays loaded across calls to `synthesis::Session`; the CLI is a single-candidate process.

## Use

Use the pinned runtime described in [portable engines](portable-engines.md). With no explicit design path, `design` automatically acquires the operation-compatible pinned VoiceDesign checkpoint for the selected engine—1.7B Q8 GGUF plus F32 codec on GGML, 1.7B 6-bit MLX snapshot on Apple Silicon MLX—never the 0.6B Base-cloning weights. Cloning settings are separate: `model.id`/`model.path` never select VoiceDesign weights, and `model.design_path`/`--backend`/`--device` govern design. Explicit paths stay local and are never replaced; offline/auto-download controls apply as usual.

Prefetch without synthesis using `models download --purpose design`. The default `models download` purpose remains `clone` for ordinary `say`/`serve`.

On Apple Silicon the default build selects its pinned MLX VoiceDesign snapshot:

- `mlx-community/Qwen3-TTS-12Hz-1.7B-VoiceDesign-6bit` at revision `ffc6545dc9cb086950aa46c6cd3db490e6ece3e1`: 13 files, 2,696,098,292 bytes (talker `model.safetensors` 2,009,267,875 bytes and `speech_tokenizer/model.safetensors` 682,293,092 bytes among them).

```sh
spqx --backend ggml --device cuda:0 --config config.toml --json design \
  "Heute probieren wir eine neue Stimme aus." \
  --instruct "A fictional adult German voice, calm and conversational." \
  --language german --seed 4242 --out candidate.wav > candidate.json
```

For custom/local assets, set `model.path` to the VoiceDesign GGUF file and `engine.codec` to its codec. Set `engine.library` only when not using the matching bundled runtime. Output is native 24 kHz mono PCM16 WAV; other configured sample rates are rejected rather than silently resampling candidates. No denoising, trimming or regeneration is performed. Existing output files are not overwritten, including files created while inference is running. No live playback is started.

The JSON records the description, spoken text, language, seed, model path, selected backend and timing boundaries. Keep the exact WAV with this recipe and independently recorded model/runtime hashes. A seed is not a portable voice identity or a guarantee across engines/devices. Saving a candidate does not promote a registry voice or enroll recognition.

## API and mode safety

```rust,ignore
let session = spqx_core::synthesis::Session::load_voice_design(
    model, description, "german", Some(4242), &engine_options,
)?;
// Reuse session.synthesize(text, sampling, audio_callback, cancelled).
```

The loaded checkpoint type must be exactly `voice_design`: GGML checks libqwen's `qt_model_type`, MLX checks `config.json` before loading weights. Base and CustomVoice checkpoints fail; changing a flag cannot turn them into VoiceDesign. The GGML request carries an instruction and seed with **null** speaker/reference/transcript/embedding/code pointers. MLX builds the official prompt: `<|im_start|>user` description `<|im_end|>`, the assistant role prefix, and a codec prefix with no speaker position, then feeds target text through the official streaming text input (first token over `codec_bos`, the rest and `tts_eos` trailing one per decode step), with a fresh vocoder state per request and the checkpoint's 1.05 repetition penalty. Reference cloning keeps its separate constructor and requires `base`. Streaming callbacks, cancellation, finite-PCM checks and conservative frame-cap status are shared.

Descriptions must be nonblank, NUL-free and at most 4096 UTF-8 bytes. A candidate must reach EOS, and be finite, non-silent and unclipped, or it is rejected unsaved. An explicit `--seed` is one deterministic attempt; if it misses EOS nothing is saved and the CLI says so. An unseeded run tries at most three recorded random seeds and reports the seed used and any rejected seeds. MLX applies a per-request seeded key sequence without touching a global RNG, so a seed request replays the same candidate. Reused descriptions sample candidates, not an automatically persistent speaker identity. For later cloning, explicitly select and preserve an audition's raw audio, matching text and provenance.

Legacy tensor-level VoiceDesign is **not implemented**; the old `Qwen3TTSModel::generate_voice_design` reports an explicit unsupported error instead of a two-second zero waveform. CustomVoice/preset support and PCA-conditioned synthesis are separate capabilities. No worker opcode was added: description-mode framed-worker startup and Voice Lab integration remain separate work. Frozen Base-model PCA and canonical-save integration are described in [frozen voices](frozen-voices.md), not capabilities of the description constructor itself.


## Verification scope

Actual Linux x86-64 RTX 4090 CUDA execution passed with:

- `Serveurperso/Qwen3-TTS-GGUF@b7ee2e8c7459c3bea99da23e3d178125a7d1713c`.
- `qwen-talker-1.7b-voicedesign-Q8_0.gguf`, 2,042,833,824 bytes; SHA256 `575610ab1ddcca4dca6bd9a64bcd859d93bbad8764f9cab24e1dbc0c51f62276`.
- The existing 647,263,104-byte F32 codec, SHA256 `b16b95557c7c7340a121757bd6855b9609e1cf4ad3fad0778b89393293ae5f3d`.
- Pinned libqwen ABI 6/source and GGML revisions from the portable-engine contract.

Two German utterances (4.88/5.04 seconds) completed with finite, non-silent, unclipped audio using one loaded session. Tests rejected Base-as-VoiceDesign and exercised pre-cancel, active callback cancellation, a two-frame cap, malformed text and a subsequent full EOS recovery. The CLI's first candidate was byte-identical to the corresponding API candidate in this fixture, including on a second bounded run. This is not cross-engine/checkpoint parity.

In the final single run, prepared-session first decoded PCM arrived at 128 ms for the first utterance and 13 ms for the post-interruption utterance; complete generation took 614/471 ms. The separate fresh-process CLI reported 689 ms preparation, 126 ms first PCM after preparation, 604 ms generation and 1.414 seconds total for 4.88 seconds of audio. These are individual callback/process observations, not p50/p95, audible playback or Voice Lab latency benchmarks. Native streaming chunk scheduling differs from MLX.

Fresh non-evicting 5 GiB leases were released after each run. The final guard sampled at most 3,826 MiB owned GPU memory (not a continuous peak), with two CPU cores and a 180-second deadline. No shared workload or service was changed. ASR matched the first passage; on the second Whisper reported one extra `Haha` (1 insertion/11 target words). ASR is a proxy, not listening approval of accent, naturalness or character.

CUDA VoiceDesign is the verified GGML target; CPU, Windows, Vulkan and GGML Metal are not qualified by these results.

Native MLX VoiceDesign was executed on an Apple Silicon M2 Ultra (macOS 15.7.9) against that pinned snapshot. A loaded description session prepared in 0.52 s; the first decoded PCM arrived 0.14-0.23 s after preparation and complete generation took 1.6-2.0 s for 4.7-6.0 s of audio with EOS. The loaded-session contract passed: Base-as-VoiceDesign rejected, pre-cancel, callback cancel, two-frame cap, malformed `NUL` text and full EOS recovery, plus an identical seeded replay from the same session. A fixed German sentence across eight seeds produced six complete candidates (4.08-4.96 s) whose large-v3-turbo ASR matched the target text exactly, and two rejected attempts. Two seeds rendered in the non-streaming text layout instead ran into the cap at 18.2 s with the speech ending after ~2 s (RMS -83 to -96 dBFS afterwards), which is why the streaming text layout is the implemented one.

The VoiceDesign token layout, seed-key behaviour and checkpoint/language/text validation are covered model-free. Voice Lab integration, canonical promotion and native PCA are separate feature work, not delivered by this CLI/API slice.

## Main integration validation (2026-10-21)

The source landing preserves both `design` and `models download`. The integrated model-free suite passed 33 tests (3 explicit native/MLX cases ignored), and portable core/CLI `cargo check` passed. A new CLI regression checks both commands and rejects missing VoiceDesign `model.path` before creating a Base cache or candidate WAV, with clean JSON stdout. No new inference or MLX VoiceDesign parity is claimed by this landing.

Structural review reported zero gating regressions: seven static dead-code findings on test functions/modules, three minor churn findings and one 61-line native acceptance-test verbosity finding. These were reviewed, not treated as a clean absence of findings; the self-contained ignored acceptance retains its separate actual CUDA evidence above. Static caller coverage is incomplete across subprocess/FFI tests.

This VoiceDesign proof does not qualify PCA or canonical saving. Frozen Base-model PCA and canonical saving are a separate, separately qualified native GGML feature described in [frozen voices](frozen-voices.md); SDK and native banks are not interchangeable.


## Reproduce

`just check-portable` and `just test-portable` are model-free. The opt-in `just test-native-design` needs fresh compute admission and explicit environment:

- `SPQX_DESIGN_ASSETS`: directory containing the pinned VoiceDesign talker, codec and a hash-checked Base talker named `base.gguf` for the negative case.
- `SPQX_DESIGN_LIBRARY`: matching trusted native library.
- `SPQX_DESIGN_DEVICE`: explicitly selected device, such as `cuda:0`.
- `SPQX_DESIGN_OUTPUT`: fresh private output directory.

At the original feature validation on 2026-10-20, local default MLX compilation was blocked by a missing Apple Metal Toolchain, after initializing the pinned submodule; portable checks and actual Linux builds passed. This is historical evidence, not a claim about current toolchain availability. No global SDK/toolchain installation was attempted. Scope remains `spqx-kazg.13.3` above `spqx-srtg.1`; broader registry/UI work remains open.
