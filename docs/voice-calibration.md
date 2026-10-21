# Vector-only voice calibration

Experimental, separate from faithful full-ICL cloning. The tested fixed ICL context
perceptually dominated speaker-vector swaps; collect identity/timbre ratings on the
actual vector-only reuse route instead. See [frozen ICL](frozen-icl.md).

## Auditions, not labels

The first private bank contains 100 fictional adult German auditions, generated from
varied VoiceDesign descriptions and re-extracted through one pinned native 0.6B Base
encoder. Every rated take uses its frozen 1024D vector, the same German passage and
seed 4242, without reference context. Source descriptions are requested variation,
not observations, and remain hidden in the rating API/UI.

Six subjective 0–100 traits: pitch, brightness, breathiness, roughness, vocal weight
and nasality. Naturalness is separate; pronunciation/intelligibility issues,
exclusion and notes are explicit. Midpoint defaults are not stored until Save.
There is no accent axis, machine trait labeling, neural training or enrollment.

Private custody: original designed WAV/text/description/seed, exact extracted and
effective vectors, raw audition WAV/text/bundle, hashes and stage receipts.
Listening AAC copies use constant gain only; raw outputs remain unchanged.
Numerical uniqueness and EOS/finite-signal checks do not establish 100 perceptually
distinct voices, native pronunciation, naturalness or useful semantic controls.

## Run and use

Add an explicit private corpus directory to the existing Voice Lab config:

```toml
rating_bank = "/private/voice-bank"
```

CLI `--rating-bank` and `VOICE_LAB_RATING_BANK` follow the existing configuration
precedence. Open `/ratings`; listen, adjust scores, choose **Save & next**. A rating
is not a canonical voice. Voice shaping, existing drafts, saved voices and preview
budgets remain separate and unchanged. No generation occurs on rating reads,
slider edits or saves. Unsaved navigation drafts live only in the page; reloading
warns before discarding them. Startup resumes at the first unrated voice.

## Shared file/API contract

`index.json` version 1 requires `mode = "vector_only"`, a bank id, text,
`target_count`, status and ready candidates. Each candidate has an opaque `cNNN`
id, seconds, raw `audio_sha256` and `listening_sha256`. Other private index fields
are not exposed. Source prompts and requested categories must not become labels.

- `candidates/cNNN/listening.m4a`: hash-bound listening copy.
- `candidates/cNNN/audition/reference.wav`: exact raw rated take.
- `ratings/cNNN.json`: versioned explicit annotation; absent means **unrated**.
- `ratings/cNNN.lock`: cross-process exclusive save lock, with pid/intent/time.

`GET /api/ratings` returns blind state, trait definitions and saved annotations.
`GET /media/rating-cNNN` serves verified AAC with byte ranges. `POST
/api/ratings/cNNN` accepts [rating-request.schema.json](../demos/voice-lab/rating-request.schema.json):
bank/audio hashes, expected revision, UUIDv4 operation id, six scores, quality,
flags and notes. Send the exact app Origin and JSON content type, including from
CLI clients. Existing exact Host/mandatory write-Origin and private-tailnet keyless
policy apply. Reachability is not conversational/session-binding authority.

Saves verify raw/listening custody, reject unknown bodies and stale revisions,
serialize through a file lock, and atomically replace the annotation. Retrying the
same normalized intent returns the existing revision; reusing its id for changed
scores fails. A later stale intent cannot roll back newer scores. Score corruption
fails closed rather than being treated as an unrated voice. Media and annotation
symlinks are rejected. No GUI-only state: agents inspect the files or use the same
HTTP operations; they must not invent human ratings. Prefer the API over editing
annotation fingerprints by hand.

A crash can leave a save lock. Inspect its recorded pid and the owning service;
only after confirming that operation is dead may the operator remove that stale
lock. There is no unsafe timeout-based lock stealing. The controller split and
broader service hardening remain tracked separately.

## Experiment gate

Freeze an 80/20 train/held-out split before reading labels. Fit PCA and a regularized
trait mapping on training data only; held-out labels do not tune hyperparameters.
Obtain blinded repeat ratings before claiming reliability. Validate held-out
voices, intermediate edits and new sentences with human listening. PCA is not
semantic by itself; requested descriptions are never substitute supervision.
Do not ship a retro-console control surface until useful audible edits are shown.

## Evidence

`just test`, `just check`, `just ratings-browser-check` in `demos/voice-lab`.
The browser check starts a fresh isolated loopback 8805 service with synthetic
non-speech audio/state only. It tests keyboard edits, zero drag-save/inference,
acknowledgement-loss retry, stale tabs, reload, unsaved navigation, AAC decode and
playback clock, Range 206, Origin rejection and 320/390px overflow. It never rates
or registers a real corpus candidate. Studio captures use a labeled read-only
synthetic fixture; browser clocks are not physical-phone playback evidence.

The private first-bank audit verified 100 complete native auditions, distinct
vectors/PCM, exact effective-vector bits, custody hashes and no ICL/promotion or
automatic ratings. Five separately admitted 20-attempt 5GiB batches completed in
697.6s summed guard time, sampled owned peak 3816MiB, all leases released. These
are batch observations, not latency percentiles or acoustic acceptance. No ASR
check or learned trait mapping is claimed.
