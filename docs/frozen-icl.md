# Frozen full-ICL voices

Explicit native prototype: freeze **speaker vector + original reference codes + matching reference text**, not the vector alone. [ADR 0003](adr/0003-frozen-icl-context.md) records the trade-off. In the preceding human-listened English-reference→German pilot, vector-only lost the accent; that is not a universal claim about embedding contents.

## Extract and replay without the source WAV

```sh
spqx --config base.toml lab extract --full-icl \
  --ref-audio english-reference.wav --ref-text-file english-reference.txt \
  --language english --out prompt.json
spqx --config base.toml lab preview "Ein neuer deutscher Satz." \
  --icl prompt.json --language german --seed 4242 --out full-icl-draft
```

`prompt.json` is a distinct `FrozenIcl` envelope (`mode: full_icl`), with an unchanged v1 speaker vector and a scoped/checksummed context. A fresh replay needs no source WAV/text files. Keep them as optional listening/migration archives; their hash is provenance, not enrollment permission or proof of transcription accuracy.

The saved audition uses bundle v2. Its audition WAV/text can be German while `icl_context.text` and codes come from the original English reference. Never replace the latter with the new audition transcript. Existing vector-only v1 documents serialize as before; `--conditioning` remains vector-only. Loading a full prompt never silently drops context or re-extracts audio.

Explicit `lab save` and `say --voice-id` retain the v2 context through the existing voqx sidecar interface. Old strict consumers see the retained sidecar sentinel and reject unsupported v2 metadata instead of cloning the audition WAV as a fallback. No automatic registration or registry-schema fork.

## Speaker edits with fixed context

Preview can combine `--icl prompt.json` with `--conditioning edited-vector.json`, or with `--space bank.json --base B --offsets=0.3,-0.2`. All scopes must match exactly. Only continuous speaker geometry changes; context code order/text/checksum stay intact. PCA banks are still fitted to speaker vectors, never flattened/interpolated discrete codec codes. This interface is implemented, but it is **not a demonstrated useful voice-morphing control**. Human results below show that fixed reference context dominates perceived speaker identity in the tested configuration; do not generate a100-voice speaker-PCA rating bank under that route.

## Human control-authority result

The user heard no useful voice changes from the four-source, two-axis ±0.75σ fixed-context PCA pilot (audio-drops `b949b68`). A stronger diagnostic then swapped entire native vectors from fictional source A (warm baritone) and B (bright mezzo), with identical model/text/top-k1 settings (`d7b88b4`):

- Vector-only: user heard male then female, with no accents.
- Fixed English full-ICL context: user heard the same woman and the same English accent in German.

This supports **perceptual ICL dominance over speaker identity** for this0.6B Base Q8 test, not a universal model claim or proof that the vector is literally discarded. The pinned native prompt builder still inserts the supplied speaker vector into its prefix before adding the reference-code sequence. Exact inputs were verified; different output hashes are not evidence of useful identity control.

Keep two capabilities separate: vector-only speaker/PCA calibration for editable timbre/identity, and frozen full-ICL reference cloning for faithful context/character reproduction. A calibrated semantic PCA bank and independent accent control are still unproven. Do not label an accent slider or silently convert one mode into the other;100 ratings cannot establish control over an input that has negligible audible authority in the chosen route.

## Closed first qualification

- Native GGML Base with pinned qwentts.cpp `5916bf7c79dbb4e66fc059cdc507a79a23a28bb4`, ABI6; no native source/ABI/worker protocol change.
- Qualified Qwen12Hz F32 codec SHA256 `b16b95557c7c7340a121757bd6855b9609e1cf4ad3fad0778b89393293ae5f3d`. Codec tensor inspection confirms2048-entry books. Context is native `[16,T]` row-major/T-fastest int32,1–512 frames, codes0–2047. Loaded codebook count is checked through existing `qt_num_codebooks`.
- Exact model/codec/encoder/dimension scope and context custody are validated. Unknown codec/layout/mode, changed text/codes, wrong widths and unsupported engines fail explicitly. MLX frozen ICL is not implemented; Apple Metal-JIT compilation is not inference proof.
- Live experimental Lab remains its current vector-only provider; this prototype does not silently switch old voices or the UI to v2.

## Proof

`just test-native-icl` is ignored/opt-in and requires explicit `SPQX_ICL_MODEL`, `SPQX_ICL_CODEC`, `SPQX_ICL_LIBRARY`, `SPQX_ICL_REFERENCE`, `SPQX_ICL_TRANSCRIPT`, `SPQX_ICL_DEVICE`, fresh `SPQX_ICL_OUTPUT`, and external accepted compute admission. Optional `SPQX_ICL_OLD_CLI` checks legacy rejection. Never point output at an existing/user directory or let a test infer permission to download/evict.

Actual CUDA0.6B Base Q8 proof: complete context export/reload, original input path removed, fresh-process CLI replay, wrong-width rejection before PCM, pre/active cancellation, cap, malformed-input recovery and retained-session reuse passed. **Original-reference PCM, frozen-context API PCM and fresh CLI WAV were byte-identical** for the same German passage under top-k1 greedy selection. Reference has16 books ×76 frames. This is exact replay evidence for that fixture, not stochastic-output/engine parity or a semantic accent-control guarantee.

Own3.35GiB/180s non-evicting lease released; guarded run9.414s, sampled owned peak3344MiB below3430MiB limit, CPU0/1. Sampling is not a continuous peak guarantee. No global canonical registration, model downloads, shared changes or neural training. Human listening confirmed the frozen full-context replay retained exactly the same accent. Subsequent editing results are below.
