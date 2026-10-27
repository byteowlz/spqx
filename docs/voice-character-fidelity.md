# Character expression and editable conditioning

Status: investigation, spqx-8s4r. The user heard substantially more expression in the original fantasy/cartoon designs than in their speaker-only reuses. Full-fidelity editing has not been established.

## What the current comparison actually changes

Original: native Qwen1.7BVoiceDesign Q8 with the complete character instruction. Reuse: native0.6BBase Q8, extracted1024D speaker-only conditioning, no instruction or reference audio codes. Both the model and conditioning mode change. This comparison cannot tell us how much each change caused the loss of character. All original WAVs/prompts are retained.

Pinned qwentts.cpp5916bf7 / public ABI6 explicitly rejects style instructions on Base and reference conditioning on VoiceDesign. It does not expose a combined reference+instruction mode, numeric Pace/Warmth parameter or a reusable VoiceDesign style-vector API. Sampling temperature is not a pace or warmth control.

Full ICL preserves the speaker vector plus codec reference codes and matching source text. The previous0.6B investigation retained accent, but fixed ICL overpowered tested vector edits; see frozen-icl.md, voice-calibration.md and ADR0003. That is a warning, not proof that every richer model/context is uneditable.

## Bounded next experiment

Use one original character first, fixed source/model/runtime hashes and explicit seeds. Preserve the source and create new immutable takes; no rerolls, cap inflation, incomplete-output trimming, enrollment or migration of old judgments.

1. Replay the original1.7BVoiceDesign prompt as control, then request slower, faster, warmer and brighter delivery through its native instruction path. These are requested prompt edits, NOT learned PCA axes or proven independent controls.
2. Run the unchanged character prompt on new German text. A fixed prompt+seed is not a frozen identity; listening must establish consistency across words.
3. Compare original reference to full-ICL0.6B reuse with original and new text. Test nonzero same-scope vector perturbations separately: preservation alone is not editability.
4. If conditioning/capacity remains limiting, qualify a matching1.7BBase model and compare vector-only versus full ICL. Re-extract its conditioning; never carry0.6B frozen vectors/PCA banks across model/encoder/dimension scopes.

Bound the first phase to ten new takes. Require fresh own truthful semantically accepted dpty admission, active/release receipts and resource guard; no service stop or peer-lease action. Existing cached models cover the first phase; no hosted model fallback or training. This is a plan, not an executed control/quality result.

## Acceptance

Retain exact prompts, requested edit, seeds, raw WAVs, model/runtime scope and conditioning for every outcome. Report EOS/finite/signal/custody checks honestly. Provide individual explicit A/B playback through the existing workflow, not just a reel. Human listening must separately confirm: preserved character/expression on new words, useful requested edits, no identity drift and no collapse to an unchanged output. Duration, ASR and recognition similarity can expose failures but do not prove these qualities. Do not advertise full-fidelity or semantic independent dials before that comparison.
