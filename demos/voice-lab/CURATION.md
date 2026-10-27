# Audition curation

Open `/curation` on the existing Voice Lab listener. Configure `curation_bank = "/private/completed-bank"`, `--curation-bank`, or `VOICE_LAB_CURATION_BANK`. Existing configuration priority and trusted-tailnet Host/Origin policy apply. This is a separate bank from `rating_bank`, with no inference, promotion or recognition-enrollment operation.

## Human contract

Individual playback, explicit previous/next selection, keep/reject/unsure and optional notes. Initial decision is null, not unsure or a numeric midpoint. Notes-only partial saves are allowed; the reviewed count includes deliberate decisions only. Clearing a decision is a draft until Save. Choice changes never write. Save only and Save & next are explicit operations. Switching/reloading with a draft asks before discarding it. Playback stops on candidate changes. A lost acknowledgement keeps the exact save intent frozen for idempotent retry or explicit reload/reconciliation; a stale revision keeps the draft rather than overwriting someone else's review. No browser storage is authoritative.

## Shared API and CLI

- GET `/api/curation`: read-only version1 state, bank ID, requested text, candidate IDs/durations/audio/vector hashes/media URLs and saved reviews.
- POST `/api/curation/cNNN`: `curation-request.schema.json`; exact Origin required, including CLI HTTP clients. Requires bank/audio hash, expected revision, fresh operation UUID, decision and notes. Same UUID/body retry returns the existing receipt; changed content with that UUID is rejected.
- GET `/media/curation-cNNN`: allowlisted, hash-checked AAC through the existing Range handler. Private vectors, paths and source prompts are not served.
- `bun curation-cli.ts read --bank DIR`: genuinely read-only JSON; no review directory is created.
- `bun curation-cli.ts save --bank DIR --candidate c001 --input intent.json`: same validation/store as the UI. `--input -` accepts stdin. Errors are JSON on stdout with stable codes and nonzero exit. `--help` names side effects and recovery.

Readiness does not grant authority: CLI access is not permission to invent human decisions. Use isolated fixtures for proof, and change real reviews only on the user's explicit instructions.

## Multiple banks and source comparison

Keep `curation_bank` as the original default. Add operator-configured extra banks with `[curation_banks]` and `characters = "/private/character-bank"` in TOML.

The bank selector uses `?bank=characters` on `/curation`, its state/save API and media URLs. Unknown keys fail closed; keys never become client-provided filesystem paths. Existing callers without a key retain the original bank. Each bank keeps its own review directory; repeated `cNNN` IDs cannot cross-save. The selected bank is shareable in the URL, not authoritative browser-only state. Switching with a draft asks first; an unknown save locks both bank and candidate.

Source comparison requires `design_audio_sha256`, `design_listening_sha256`, and `design_seconds` in the index. Raw `candidates/cNNN/reference.wav` and AAC `design-listening.m4a` are independently hash checked, distinct from the Base audition. A separately namespaced `source_media` URL plays the original design; review inputs/Save are disabled and the editable draft is kept. Switching back restores editing. Reviews always bind editable/Base audio; source hashes and variant fields cannot masquerade as that target. No old review format or annotation migration occurs.

## State, custody and recovery

Input is a completed version1 index with SHA-bound raw WAV, listening copy, native frozen document and1024D effective-f32-vector hashes. Save verifies these artifacts and their matching requested text; mismatches fail closed. Candidate IDs are confined, symlinks refused. No generated/described trait is treated as a human label.

Only `BANK/curation-reviews/cNNN.json` is authoritative review state. Saves use exclusive per-candidate locks, fsynced temporary files, atomic rename and a prior revision at `cNNN.previous.json`. A corrupt/stale saved record is not silently replaced. An abandoned lock is an operator-inspection condition; do not remove an unknown active lock. For recovery, inspect the owning process and candidate hashes, stop that service if necessary, preserve the suspect file, then restore the prior version deliberately. Never blindly retry an uncertain changed intent.

For a portable backup, preserve the bank and review directory together. File-copy roundtrip is tested; review contents have no developer-checkout paths. A bank with changed ID/audio cannot inherit old decisions. Rating-bank files, old drafts/jobs, native models and canonical registry remain outside this module's writes.

## Proof and scope

`just curation-check` runs12 model-free tests/61 assertions and builds the server. `just curation-browser-check` uses an ephemeral loopback listener, synthetic tones and a temporary bank; it never changes real reviews or calls inference. Its39 checks cover actual playback, explicit saves/reload, no initial/default decisions, unsaved-navigation protection, lost-response idempotency, stale drafts, API-to-open-UI visibility, keyboard uncertainty, candidate playback fencing and1280/390/320px layouts, independent bank switching, URL reload, original-source playback, source-save exclusion and preservation of old-bank decisions. Both light/dark captures were inspected. Screen-reader, physical-phone and human acoustic acceptance remain unverified.

The current dirty checkout's full suite passed46 tests/248 assertions, including another writer's uncommitted rating work; that is not a claim those changes ship here. The preserved Linux runtime plus this isolated overlay passed44 tests/224 assertions and built on Bun1.4.2. macOS uses Bun1.4.4. An idle single-port update preserved old source/config as rollback inputs, all protected old-bank/app-data hashes, jobs and drafts; no extra review port or GPU service change. Do not repeat an uncertain deployment or kill an unfamiliar process. macOS source packs use `COPYFILE_DISABLE=1 tar ...` so AppleDouble companions are not mistaken for TypeScript tests on Linux. Directory-entry assertions sort the OS-unspecified enumeration order.

The browser check found and fixed a real pending-save status race; its first fieldset-disabled assertion was corrected to check the actual radio/notes/navigation controls. One bounded visual correction batch removed an empty status stripe, separated Notes, compacted phone playback, themed the JSON link and native dark controls. The detector's readout-padding warning was addressed with8px inset. No independent reviewer model is configured; finish inspection was explicitly substituted inline, not independent approval.

## Visual authority

Mode: Operate. Product/surface: SPQX Voice Lab / candidate curation. Variant: inherited Chalk workbench anatomy with the exact neutral palette from Studio `spqx-voice-studio-r4` source `9902c69655064afdab89b3ce058dfafb35528612`. No green, orbs, new imagery or decorative webview. Existing tokens/typography, conventional radio/audio controls and desktop-to-phone stacking are retained. Studio line `spqx-voice-curation` records bounded functional proposals and actual synthetic Web evidence. r2 adds bank selection and editable/source comparison without changing the retained neutral workbench identity. No selected production target, native GPUI implementation or appearance approval is inferred from the user's request for a web UI.
