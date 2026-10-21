# ADR 0002: Frozen conditioning plus audition archives

Status: accepted for implementation; backend and perceptual qualification remain separate.

## Context

The user explicitly wants to keep both the exact TTS conditioning vector and its audition WAV, then continue editing genuine PCA directions with sliders/knobs. A prompt or seed does not define a persistent voice. Re-extracting a vector from a saved waveform can move the chosen conditioning point; a low-rank reconstruction can discard the original voice's residual. The existing Lab retained audio and reconstruction ingredients, but not a standalone effective input vector.

The shared voqx Voice Identity already supplies canonical `voice_id` and separate synthesis/recognition profiles. Creating another identity store would fragment that contract. Model families, vector widths and recognition embeddings do not establish TTS-space compatibility.

## Decision

A saved spqx audition keeps **the complete effective conditioning vector, exact WAV, matching text/language, provenance, and optional fitted PCA bank plus offsets**. The vector is the compatible runtime input; the WAV remains an archive/portable source. Frozen loading never silently re-encodes it, normalizes/pads/truncates vectors or switches engine/model.

Frozen v1 is raw-f32 x-vector-only conditioning, scoped by resolved engine, model and codec artifact SHA256, encoder implementation and dimension. Native validation checks the loaded checkpoint mode and dimension before generation. ICL reference codes are not silently substituted or discarded; ICL uses its own explicit format in [ADR 0003](0003-frozen-icl-context.md), never an implicit v1 upgrade. A VoiceDesign description produces an audition first; the selected Base encoder then supplies its own conditioning space.

PCA is fitted centered SVD over independent, same-scope source vectors. Edits are `base + sum(offset_sigma * fitted_scale * direction)`, retaining the entire residual. Zero offsets preserve base bits. Axes have no invented pitch/age semantics, monotonicity, identity or naturalness guarantee. Experimental bounds are declared, not learned acoustic limits. A saved edited vector also retains its exact bank for reopening the controls; replacing the bank cannot reinterpret an existing voice.

Numeric JSON uses `serde_json/float_roundtrip`. Without it, parsing a fitted f64 basis can shift a bit and invalidate its content identity. Consumers that do not reproduce Rust's numeric serialization must copy `voice.json` bytes rather than parse/re-encode the numeric archive. Files are data, never executable pickle.

## Canonical registry seam

Explicit Save promotes a completed audition through the pinned, ML-free `voqx-core` locked/atomic store interface. Preview and fitting never register identities. No display-name/embedding auto-merge occurs; attaching a performance to an existing identity requires its explicit `voice_id`. Recognition and wake profiles are untouched.

The voqx reference remains normal audio + transcript. The spqx-owned `spqx:conditioning-v1` reference tag declares that the reference directory also contains `voice.json`. That versioned sidecar owns the exact vector, bank and provenance; there is no voqx registry-schema fork. The reference and engine-agnostic source recording can point to the same archived WAV/text without duplicating bytes. Missing/corrupt tagged sidecars fail rather than falling back to ordinary cloning.

An optional `spqx:save-op:<uuid>` tag is an engine-owned durable retry key. Within the registry write lock, replay of the same operation must identify the same bundle and return the existing id. Reusing an operation for different contents fails. This is transaction idempotence, not identity similarity or authorization. Normal separate save intents may create distinct identities even with identical display names/audio.

## Consequences and proof

First qualified runtime target is native GGML Base. Old SDK and native PCA banks remain different scopes; native banks are fitted from actual native extraction of the kept source recordings, never by assuming equal-width SDK vectors are portable. MLX data declarations do not imply an implemented/qualified frozen-input path.

The existing Web look is retained: actual PCA controls, explicit Preview, A/B, stale-settings disclosure, and save of the completed take. Native providers can expose an explicit reusable-voice Save; SDK providers keep draft-only saving. Both capture effective vectors and keep WAVs. Legacy drafts remain archives and are never relabelled into a new space. The app's user-directed keyless trusted-tailnet policy remains, including exact Host/Origin and resource guards; enabling native canonical Save grants that synthesis-only action to reachable clients, not recognition enrollment or conversational authority.

Model-free tests cover residual/neutral identity, rank/finite/scope failures, lossless reopen, custody, symlinks, canonical rename/reopen and retry identity. Actual admitted native tests must prove frozen synthesis after process/registry reload and correct failure for mismatched scope/dimension. Human listening remains necessary for useful controls, character, accent and naturalness. Compilation, ASR agreement and callback timing are not perceptual/platform parity.
