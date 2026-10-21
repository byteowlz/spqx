# Voice Lab

Experimental real-PCA workbench: explicit previews, A/B listening and exact WAV/vector persistence. No prompt-backed sliders, nearest-neighbor playback, implicit enrollment or neural-weight training. See [frozen voices](../../docs/frozen-voices.md) and [ADR 0002](../../docs/adr/0002-frozen-conditioning-voices.md).

## Run

Requires Bun. Live previews require Linux CUDA, `dpty`, GNU `timeout` and the selected provider's existing assets. Nothing automatically installs models or substitutes providers.

Native provider (qualified GGML Base scope, existing spqx whole-pipeline CLI):

```sh
bun install --frozen-lockfile
bun server.ts --host 127.0.0.1 --port 8804 \
  --provider native-spqx --profile /private/native-pca-profile \
  --native-cli /path/to/spqx --native-config /private/base.toml --enable-preview
```

The native profile contains `profile.json`, `space.json` and `raw/` listening clips. `lab space` must agree with the profile's model hash, backend, dimension, two components and ±0.75 bound. It must come from native extraction, not an assumed-compatible SDK vector. Optional `--native-registry /private/test-registry` isolates explicit canonical saves; otherwise voqx's canonical registry is used.

SDK provider remains explicit and draft-only. It additionally requires `uv`, the existing qwen-tts0.1.1 environment and the SDK PCA probe artifacts:

```sh
bun server.ts --provider experimental-sdk --profile /private/sdk-pca-probe \
  --python /private/sdk-venv/bin/python --enable-preview
```

Configuration: CLI > explicit `--config` > `VOICE_LAB_*` environment > local `config.toml` > global XDG `spqx-voice-lab/config.toml`. First run creates default config and `schema.json`. Environment fields include `HOST`, `PORT`, `PROFILE`, `PYTHON`, `PROVIDER`, `NATIVE_CLI`, `NATIVE_CONFIG`, `NATIVE_REGISTRY`, `RATING_BANK`. Default listener is loopback, previews disabled; tailnet publication needs an explicit bind. Data defaults to the XDG data directory with private filesystem permissions.

The user explicitly chose keyless trusted-tailnet access: anyone reaching this listener can read drafts/media and request bounded previews; native mode also exposes explicit synthesis-only canonical Save. No login, pairing, bearer or recognition/session/tool authority is implied. This is not a general internet-public mode or another app's auth policy. Exact Host/Origin checks remain. No private/real-person upload endpoint.

## Behavior and limits

- Three source voices, two genuine centered-SVD directions, ±0.75 fitted standard deviations. Experimental 1024D model-scoped controls, not calibrated pitch/age knobs or cross-backend geometry.
- No rendering on drag. Preview captures one recipe. Later edits never relabel its audio. Save copies the completed audition, not current unrendered sliders or regenerated audio.
- Native drafts capture exact provider JSON bytes, full effective vector, fitted bank/recipe and unchanged WAV/text. SDK drafts capture the effective BF16 tensor as f32 values, not just reconstruction ingredients. Archived earlier-space audio stays playable without applying its coordinates to the new bank.
- Native **Save reusable voice** explicitly registers through voqx; durable retry keys return the same identity after an interrupted response. No name/speaker auto-merge, recognition enrollment or training. SDK **Save draft** remains noncanonical.
- Single flight, default12 admitted previews/day. Each preview obtains/releases its own non-evicting reservation: native4GiB, SDK3.25GiB. CPU2, independent180s timeout plus8s kill window below240s lease. Native additionally samples owned VRAM and enforces the4GiB bound; SDK allocator ceiling is2.6GiB. Busy/admission/provider failures are explicit, never fallback audio or shared workload eviction.
- Both providers use bounded cold processes; no idle model/GPU retention. Closing a page does not cancel unrelated work. Native integration here does not implement persistent warm editing or MLX frozen inference.

## Agent/API access

GET `/health` and `/api/state` need no credentials. Every write requires the exact nonsecret app `Origin`, including CLI clients. Typed bounded bodies, allowlisted media/realpath/symlink/range checks remain; paths/models/commands cannot be supplied over HTTP.

- POST `/api/preview`: `{ "base":"A", "axes":[0.3,-0.2], "space":"<current space>" }` → job.
- GET `/api/jobs/<id>` → recorded recipe and queued/running/ready/failed state.
- POST `/api/drafts`: `{ "job":"<ready id>", "name":"My draft" }` → exact draft.
- POST `/api/voices`: same body → exact draft plus explicit canonical save, native provider only. Failure retains the draft; retry the same intent instead of generating again.
- GET `/media/<allowlisted-id>` → byte-range audio. `/api/unlock` is retired.

## Manual voice calibration

Set `rating_bank = "/private/voice-bank"` (or `--rating-bank`) and open `/ratings` for blind six-trait ratings, two discrete judgements (voice presentation and age band, each with an explicit Unclear) and separate naturalness, exclusion and notes. Save explicitly; no generation, machine labels, training or registry promotion. `GET /api/ratings` and `POST /api/ratings/cNNN` share the versioned files, exact-audio bindings, revision checks and retry ids. Source prompts are not exposed. See [calibration contract](../../docs/voice-calibration.md) and [request schema](rating-request.schema.json).

## Proof and visual status

`just test` runs 32 tests/163 assertions including effective-vector custody, lossless native numeric documents, explicit canonical save/retry, media protections, single-flight and incompatible provider/admission failures. `just check` builds the Bun server.

`VOICE_LAB_URL=http://127.0.0.1:8805 just frozen-browser-check` intentionally requires a fresh isolated native test service/registry. It performs one admitted real render, keyboard PCA edits, an in-flight change, browser decode/playback-clock checks, exact canonical save/retry/reopen, Range206 and Origin denial. Do not run it against user data. `scripts/capture.ts` captures only read-only synthetic visual fixtures.

`just ratings-browser-check` starts its own isolated loopback8805 service with synthetic non-speech audio. It also compiles the plain-JS rater, then tests explicit rating saves/reload, required categories, hidden requested attributes, byte-for-byte survival and explicit completion of pre-category ratings, lost acknowledgements, stale tabs, keyboard edits, ranges and phone widths without inference or real labels.

r4 extends the existing r3 look with manual ratings; no retro-console redesign. r3 preserves the existing r2 anatomy/tokens. Studio line `spqx-voice-lab` has no selected production target; these remain exploratory proposals and Web evidence. Native CUDA execution, synthetic screenshots, headless audio-clock advancement, physical-phone behavior and human listening are distinct evidence. Prototype controller/quota/shutdown hardening remains tracked separately; no blanket release/security approval.
