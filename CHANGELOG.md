# Changelog

## Unreleased

- Automatically fetch missing revision/SHA256-pinned GGML or MLX model profiles on CLI first use; add `spqx models download`, offline controls and configurable cache selection. Serialize concurrent downloads and atomically commit only verified files. No hosted inference, voice downloads or weights embedded in runtime releases.
- Compile CUDA candidate kernels for both SM86 (RTX3070) and SM89 (RTX4090). Verify the complete CUDA12 bundle on an RTX3070, including cold automatic model acquisition, English/German PCM, cancellation/fencing and recovery.
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
