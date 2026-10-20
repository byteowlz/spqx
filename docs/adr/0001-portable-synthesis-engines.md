# ADR 0001: Portable whole-pipeline synthesis engines

Status: accepted for implementation; platform validation is separate.

## Context

The current CLI and worker duplicate MLX reference preparation and contain Unix-only descriptor handling. Existing spqx-srtg selects GGML/qwentts.cpp for non-Apple systems after measured CUDA/CPU experiments; the portable interface has not been wired. MLX remains the preferred Apple Silicon engine. Per-operation tensor ports would duplicate an already complete GGML pipeline.

## Decision

Share a prepared whole-pipeline session in spqx-core between CLI and worker. Preserve native MLX behavior; use the pinned libqwen ABI for GGML, with explicit engine/device/model/codec/library selection. The binary framing and request IDs remain unchanged. Backend selection cannot rebind Foxline sessions or grant tool authority.

MLX is effective only on Apple Silicon, even when the Cargo feature is enabled elsewhere. Portable-only builds must not pull MLX or libtorch. The default distributed desktop target is macOS/Linux/Windows, with CPU and optional GPU backends; browser/mobile, 32-bit targets and untested accelerators are not automatically supported claims.

Load only explicitly selected or executable-relative native libraries. Validate the pinned native build before invoking layout-sensitive ABI functions. Do not call foreign default-parameter writers against a potentially different struct layout. The native engine owns model execution; Rust owns bounded audio handling, output framing and cancellation fencing. Explicit unavailable devices fail rather than silently changing engine or using a hosted service.

The existing spqx worker remains the process/transport boundary. Direct libqwen binding avoids introducing a second HTTP service or duplicating request-ID/cancellation transport through tts-server. In exchange, spqx owns ABI pinning, callback safety and platform-native library packaging. This does not merge other speech engines or their licensing boundaries into spqx.

## Consequences and proof

Runtime libraries are release artifacts alongside the CLI, not a Python service. Packaging needs platform-specific builds, source/license attribution and ABI matching. Keep existing MLX builds working and provide model-free feature/config/contract tests, Linux CPU build proof, actual CUDA synthesis/recovery and target-native Windows validation. A successful compile or upstream capability list is not model/accelerator parity. Track integration in spqx-srtg.1 and packaging in spqx-z9ng.
