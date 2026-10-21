# Frozen voices and PCA

Keep the voice you actually auditioned: **exact vector + WAV + matching text + model scope**, with the fitted PCA bank/settings if edited. A prompt/seed alone is not the saved voice. See [ADR 0002](adr/0002-frozen-conditioning-voices.md).

## Describe once, then freeze

Use `spqx design` with an actual VoiceDesign checkpoint to find an audition. Keep its exact spoken text. Select a production **Base** model/codec and use its encoder once:

```sh
spqx --config base.toml lab extract --ref-audio audition.wav \
  --ref-text-file audition.txt --language german --out source.json
spqx --config base.toml lab preview "Ein neuer deutscher Satz." \
  --conditioning source.json --seed 4242 --out draft
```

Listen to this Base-model preview before saving it: converting a designed recording into another model's conditioning is not a guarantee of exact character. Future synthesis can use the saved vector directly without reference encoding.

## Real PCA controls

Extract separate vectors for independent source voices under **the same** model/encoder scope, then fit a bank:

```sh
spqx lab fit --source A=A.json --source C=C.json --source D=D.json \
  --components 2 --max-offset 0.75 --out space.json
spqx lab space space.json
spqx --config base.toml lab preview "Heute probieren wir eine Stimme aus." \
  --space space.json --base A --offsets=0.3,-0.2 --seed 4242 --out edited-draft
```

These are real centered-SVD principal directions, not prompts or nearest cached recordings. Offsets use fitted sample standard deviations. The base residual survives every edit; zero offsets are exact identity. Three voices provide at most two nonzero components: this is a small experimental space, not calibrated pitch/age controls. The default ±0.75σ bound is experimental, not an acoustic-quality guarantee.

## Save and reuse by canonical id

```sh
spqx lab save --draft edited-draft --name "My synthetic voice"
# Example returned id; always use the actual result:
spqx --config base.toml say "Ein anderer Text mit derselben Konditionierung." \
  --voice-id vox-my-synthetic-voice --language german --no-play --out next.wav
spqx lab list
```

`--registry PATH` can select an isolated registry for tests. Without it, voqx's canonical `$XDG_DATA_HOME/voice-registry` is used. Save creates a synthesis-only identity; it does not enroll recognition, train weights or join identities by display name. `--voice-id` on `lab save` explicitly attaches another performance; it does not rename the identity. Multiple performances are selected with `say --reference-id`; the consumer never silently chooses a different scope.

The CLI still supports old named-reference `--voice` voices. Canonical `--voice-id` is a separate, mutually exclusive selection. Loading a tagged frozen performance requires its intact sidecar and compatible configured model/backend: there is no reference/model/cloud fallback.

## What is stored

A private draft directory contains:

- `voice.json`: complete effective raw-f32 vector, model/codec hashes, encoder namespace, language, seed/provenance, and optional exact PCA bank/recipe.
- `reference.wav`: unchanged completed native float32 audition, not regenerated or DSP-processed on Save.
- `reference.txt`: exact matching input text.

`spqx lab inspect draft` validates custody and the PCA recipe/vector relationship. `spqx lab schema` prints the JSON Schema. The canonical registry points to a verified copy of these assets under its identity/synthesis directory. Keep numeric document bytes intact; JSON implementations that alter float serialization or minus-zero can invalidate bank identities.

The engine supports raw frozen vectors without a WAV at runtime, but this product keeps the WAV for listening, provenance and explicitly approved migrations. A complete ICL prompt additionally needs codec codes and matching original text; frozen v1 deliberately uses x-vector-only semantics. The distinct [full-ICL prototype](frozen-icl.md) adds explicit extraction/replay and saved audition bundle v2 without changing v1 or the live Lab. Different checkpoints/quantized artifacts/backends are separate scopes even if dimensions happen to match.

## Voice Lab

The existing slider/A-B workbench supports explicit native provider selection, frozen capture and synthesis-only canonical Save. SDK mode remains explicit and draft-only; it captures the effective BF16 tensor as f32 values rather than only reconstruction inputs. No inference occurs on drag. Changes made while a preview runs do not relabel that preview, and Save copies the completed take rather than current unrendered settings. Old different-space drafts remain playable archives without applying their coordinates to the current bank.

The native provider reuses the spqx whole-pipeline CLI/session engine; it is not a second warm SDK service. It uses bounded **cold native processes** per explicit preview and releases GPU memory afterward. Do not describe it as persistent warm-worker reuse or compare unlike model scopes as parity. Desktop/phone Web evidence, native PCM execution, actual physical playback and human sound quality are separate gates.

Current native qualification is GGML/CUDA. MLX VoiceDesign and MLX frozen-input runtime remain explicitly unsupported follow-ups, despite successful scoped Metal-JIT compilation. The UI keeps the existing exploratory look; no production visual target has been selected. Model/data files, vectors and recordings remain outside git.
