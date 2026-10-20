# Voice Lab

Small experimental PCA workbench, registered separately in Owlznest. Real model-conditioning edits, explicit bounded previews, A/B listening and exact-audio draft persistence. No prompt-slider substitution, nearest-neighbor playback or neural-weight training.

## Run

Requires Bun, Linux CUDA for live previews, `dpty`, GNU `timeout`, `uv`, an existing qwen-tts0.1.1 environment and the external0.6BBase PCA probe artifacts. No automatic model installation or cloud fallback.

```bash
bun install
bun server.ts --host 127.0.0.1 --port 8804 \
  --profile /private/path/to/pca-probe \
  --python /private/path/to/sdk-venv/bin/python --enable-preview
```

Default configuration is created at `$XDG_CONFIG_HOME/spqx-voice-lab/config.toml` or `~/.config/spqx-voice-lab/config.toml`, with `schema.json`. Precedence: CLI, explicit `--config`, environment (`VOICE_LAB_HOST/PORT/PROFILE/PYTHON`), local `config.toml`, global config. Default listener is loopback; publishing needs an explicit tailnet bind. Registry inventory does not create a listener, HTTPS route or ACL.

State is stored under `$XDG_DATA_HOME/spqx-voice-lab` or `~/.local/share/spqx-voice-lab`, with private filesystem permissions. On2026-10-20 the user explicitly requested removal of the operator key. This instance now trusts reachability to its loopback/tailnet listener: anyone who can reach it can view its drafts/media and request bounded previews. No key, login, pairing, session storage or bearer header is needed. Listener/ACL/publication settings are unchanged; this is not an internet-public deployment mode or a policy for other apps. No private/real-person upload support.

Open the app URL directly, or `VOICE_LAB_URL=http://<tailnet-ip>:8804 just open`. Older browser credentials are discarded on reload; the legacy unlock endpoint is removed.

## Behavior and limits

- Three sources, two genuine PCA directions, signed offsets limited to±0.75 fitted standard deviations. Fixed German passage/seed. This is a toy model-scoped1024D space, not calibrated pitch/age controls or a universal Qwen embedding.
- Slider movement does not render. Preview captures its own settings; later edits never relabel that audio. Saving uses the completed job's waveform/text/recipe, not current unrendered sliders.
- One in-flight job per instance; default12 admitted previews/day. Every live preview obtains its own non-evicting3.25GiB reservation and releases it. GPU unavailable is explicit; nothing is stopped or silently substituted.
- Worker uses raw conditioning, no L2 normalization, CUDA BF16 with2.6GiB allocator ceiling, CPU2. Independent180s timeout plus8s kill window is below the240s lease TTL. Closing a page does not cancel the owned job; it finishes boundedly. No shared/delegated work cancellation.
- Drafts have opaque IDs, collision checks, atomic directory promotion, exact raw reference+text, numeric basis/conditioning and model/recipe hashes. `canonical_voice_id` remains null; no recognition enrollment or canonical promotion is claimed.
- Experimental SDK provider only. Native spqx MLX/GGML integration stays with the existing portability seam; no native engine/core/worker code is changed by this app.

## Agent/API access

Read `/health` and `/api/state` without credentials. All writes still require the exact app Origin; CLI clients send the nonsecret `Origin: http://<tailnet-ip>:8804` header too. Missing/null/foreign Origin is denied, even with a retired bearer. Bodies are bounded and typed, no arbitrary paths, commands, models or remote URLs.

- `POST /api/preview`: `{ "base":"A", "axes":[0.3,-0.2], "space":"<current basis SHA256>" }` -> queued job.
- `GET /api/jobs/<id>` -> queued/running/ready/failed and recorded recipe.
- `POST /api/drafts`: `{ "job":"<ready id>", "name":"My draft" }` -> persisted draft, not registry enrollment.
- `/media/<allowlisted-id>` supports byte ranges for sources, generated previews and drafts without credentials. Arbitrary paths and symlink escapes remain denied.

## Proof and visual status

`just test` exercises strict recipes, finite WAV validation, credentialless actions/state/media, retained Origin/host checks, removed legacy unlock, no inference on reads, single-flight, collision-safe exact saves, restart/receipt-file handling and traversal/symlink/range denial. `just check` builds the Bun server. The r1 proof ran one actual admitted GPU preview at(+0.30,-0.20), changed sliders in flight, verified recorded settings, A/B, exact save/reload and decoded browser audio. Current `scripts/browser-check.ts` verifies a fresh credentialless browser, live recipe validation, keyless media/ranges and save/reload using the existing synthetic preview, without spending GPU on this access-only change. `scripts/capture.ts` captures1280/390px synthetic-only fixtures, never the owner's live draft list. No human audibility/physical-phone claim.

Studio line `spqx-voice-lab`, variant r1: bounded exploratory proposal and Web evidence, not selected production design. Canonical tokens are copied unchanged from Studio's generated Oqto CSS; no new visual world. The first mobile capture was invalid and discarded; final captures use settled, software-rendered independent viewports. No independent finish reviewer was run. Mechanical detector warnings about token-based insets were checked against actual padded renders rather than treated as zero-debt approval. Prototype controller complexity remains a follow-up before product/native integration.
