# Changelog

## Unreleased

- Add blind manual voice-calibration ratings to the experimental Voice Lab: six traits, separate naturalness/flags/notes, exact audition bindings, explicit atomic saves, stale-tab protection and idempotent retries. Optional `rating_bank` config; existing native preview/save behavior and keyless Host/Origin policy retained. Private100-candidate vector-only bank technically verified; semantic controls, human acoustic acceptance and retro-console work remain unqualified. See `docs/voice-calibration.md`.

- Add explicit native full-ICL freezing: speaker vector plus checksummed original reference codes/text, distinct from unchanged vector-only v1. `lab extract --full-icl` and `lab preview --icl` retain context without a source-WAV runtime dependency; optional same-scope speaker/PCA overrides leave context intact. Bundle v2/canonical consumption fail closed for incompatible or legacy readers. Exact native original/frozen/fresh-CLI replay and interruption/recovery tests passed for one greedy fixture; human accent/PCA qualification and live Lab integration remain separate. See `docs/frozen-icl.md` and ADR 0003.

- Add engine/artifact-scoped frozen TTS conditioning and genuine centered-SVD PCA fitting with residual-preserving offsets. Keep exact audition WAV/text, effective vectors and reopenable PCA banks; fail on incompatible scope instead of normalizing, re-encoding or substituting engines. Add `spqx lab` extraction/fitting/preview/inspection/schema/explicit canonical-save operations and `say --voice-id` consumption through the existing voqx registry. Canonical saves are synthesis-only and retry-idempotent; existing named voices remain supported.
- Extend the experimental Voice Lab's existing sliders/A-B flow with standalone effective-vector capture, lossless native numeric archives, explicit native GGML provider and reusable-voice Save. Preserve the user-directed keyless Host/Origin/resource/quota guards and old draft audio. Native previews are bounded cold native processes, not a second warm SDK service; MLX frozen-input/VoiceDesign runtime and persistent warm editing remain separate work. Existing exploratory styling is retained; no production visual selection is implied. See `docs/frozen-voices.md` and ADR 0002.

- Add native description-to-new-voice generation through `synthesis::Session::load_voice_design` and `spqx design`, using the existing pinned GGML engine. Validate the loaded VoiceDesign checkpoint, omit all reference/preset conditioning, expose seed/recipe timing metadata and reject incomplete/silent candidates without overwriting saved WAVs. Actual Linux CUDA German synthesis and cancellation/cap/recovery passed; MLX/other-device VoiceDesign, framed-worker startup, Voice Lab integration and registry promotion remain separate. Replace the legacy VoiceDesign zero-PCM placeholder with an explicit unsupported error. See `docs/voice-design.md`.

- Add a shared prepared synthesis session and a pinned GGML/libqwen whole-pipeline engine behind the CLI and existing worker protocol; keep MLX on Apple Silicon.
- Add explicit engine/device/codec/runtime-library config and flags, fail closed for unavailable requested devices, and cache GGML reference conditioning across requests.
- Make portable-only builds independent of MLX/libtorch/playback. Restrict effective MLX compilation to Apple Silicon; add cross-platform CRT framing and Windows stdout-handle synchronization.
- Honor cancellation of native final partial audio chunks, including after EOS; check late cancellation before flushing worker buffers.
- Bound CLI synthesis to 1,536 frames, propagate resampling failures, and treat truncated output as failure. Legacy tensor diagnostics/examples now require `legacy-tools` with their native engine feature.
- Add model-free platform CI, protocol/config/stdout regressions, a pinned native-runtime build helper, and qualification notes in `docs/portable-engines.md`. Linux CUDA/CPU, Windows x64 CPU and actual integrated MLX English/German worker checks passed.

- Verify Windows x64 CPU English/German synthesis, CLI JSON/WAV, cancellation/fencing and malformed-input recovery; add an opt-in cross-platform native-worker test and a Windows CPU build recipe. CPU-only VM results are functional proof, not real-time performance or playback qualification.

- Add complete-bundle builders, relocation-test overrides, integrity manifests, installers and original dependency sources/notices. Release builds omit legacy encoding/API dependencies; the core library keeps them behind its default `legacy-api` compatibility feature. Include the exact MLX metallib rather than depending on the build tree. Models/recordings remain separate and are never implicitly installed.

- Validate ELF/PE/Mach-O architecture before labeling binaries, pin the hosted MLX build to an ARM64 runner, and reject dirty native checkouts rather than publishing mismatched corresponding sources.
- Normalize ZIP timestamps for vendor files predating 1980, reject unrecorded bundle files, and resolve installed Linux command symlinks before finding sidecar libraries. Cover these with model-free packaging/installer regressions.

Migration: standalone `--device` now defaults to `auto` and is honored rather than ignored. Existing MLX deployments that explicitly passed a CUDA compatibility value must select auto/Metal instead. Release candidates now have explicit macOS MLX, Linux CPU/CUDA12-SM89 and Windows x64 CPU layouts; untested platforms and broader quality/playback are not implied.
