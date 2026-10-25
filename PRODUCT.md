# SPQX Voice Studio

<!-- impeccable:product-schema 1 -->

## Platform

Native desktop: macOS, Linux and Windows, implemented with GPUI. These are all in the initial GUI scope, not macOS-only with unspecified later ports. The stock web/iOS/Android platform enum does not describe this desktop target; browser proposals are review artifacts, not the shipping platform.

## Stack

Rust/GPUI, with platform-specific desktop adapters. Reuse eaRS native companion patterns and SPQX's independently usable CLI/worker contracts. Do not embed a web shell solely to render an orb. Exact component versions and cross-platform runtime qualification remain implementation decisions.

## Users

The owner's everyday use comes first. The user explicitly chose personal-first over general-user onboarding on 2026-10-21. General-purpose distribution remains a goal, but do not expand the first version into a public service or complex onboarding product.

## Product Purpose

A compact local voice tool: manage voices, turn text into audio, listen/export, and read deliberately selected text from other applications. Advanced voice exploration belongs in a separate Voice Lab mode.

Success is a useful terminal-free path from voice/text to audio without becoming a DAW, marketplace or conversation platform.

## Operating Context

- Main window: voice library and text-to-audio creation/playback/export.
- Voice Lab: source selection, genuine model-scoped PCA changes, explicit Preview, A/B listening and exact-audition save.
- Compact player/menu-bar or tray access: system read-aloud without raising the full studio.
- Selected-text capture is platform-specific. macOS Services is the first investigated adapter; other platforms require their own supported mechanisms. Clipboard reading is explicit, never passive monitoring.

## Capabilities and Constraints

- Small, attractive GPUI GUI, inspired by ElevenLabs voice orbs and informed by Foxline/eaRS, as requested by the user.
- User preference confirmed 2026-10-21: avoid excessive menus. Put frequent actions directly on the surface; disclose less-used settings without menu stacks.
- User chose interactive code-first exploratory designs, not image-first compositions.
- Local inference and existing pinned model downloads/offline controls; no implicit cloud fallback or private-data upload.
- Preserve cancellation, stale-audio fencing and honest downloading/loading/generating/playing/error states.
- Use controls only when the selected backend supports them. Native GGML PCA evidence does not establish MLX parity.
- Keep voqx canonical voice identity distinct from engine/checkpoint conditioning and noncanonical drafts. No GUI-only identity registry or implicit enrollment/training.
- Save the completed audition's exact artifact/recipe, not current unrendered slider values. No inference on slider drag.
- Developer ID signing/notarization is a required macOS distribution goal; actual signed runtime and notarization remain unproved.
- Portable engine evidence is not proof of portable GUI rendering, selection capture or installers.
- No approved production GPUI visual target exists yet. Studio proposals and their approval are separate from this product scope.

## Evidence on Hand

- SPQX portable engine/worker and automatic model acquisition contracts, main 0b45c15 at research time.
- eaRS `apps/ears-ui`: GPUI companion/native overlay, tray, role-based themes and reduced-motion patterns.
- Foxline orb source: Three.js reference, not a GPUI implementation; its current external texture dependency must not be inherited silently.
- Existing experimental Studio `spqx-voice-lab-r3` and native frozen-PCA feature-branch contracts. Browser screenshots are not native acceptance or acoustic approval.
- Local research report: `~/.local/share/spqx/gui-research/REPORT.md`, tracked by spqx-9ttq; follow-on work spqx-1we0.
- User confirmation on 2026-10-21: personal-first audience; all three desktop operating systems in initial scope.
- [Executed native feasibility probe](experiments/gpui-services/proof.md), 2026-10-25: ad-hoc hardened `.app`, strict signature verification, native application loop and one owned Unicode handoff passed without extra entitlements. Not OS Services routing, Developer ID/notarization or a signed MLX product bundle.

## Product Principles

1. Everyday creation first; advanced voice experiments remain explicitly separate.
2. Local/private by default, with deliberate user actions and no invented capabilities.
3. Exact artifact custody and shared CLI/GUI state, not parallel identities or hidden rerendering.
4. Small native surface, with platform limitations stated rather than obscured.

## Accessibility & Inclusion

Keyboard operation, visible focus, textual state labels and reduced-motion behavior are baseline constraints. VoiceOver and equivalent desktop accessibility need real platform testing; GPU rendering alone proves none of them.

## Open Decisions

Visual composition, selected Studio variant/revision, orb rendering treatment, shared component choice, platform-specific selected-text integration outside macOS, and the actual signed/notarized release pipeline. None is settled merely by this file or an agent recommendation.
