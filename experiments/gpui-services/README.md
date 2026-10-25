# GPUI/Services feasibility probe

Isolated development harness, not the SPQX GUI or an approved design.
No model loading, audio playback, microphone/Accessibility permission request,
general clipboard access, background agent or private text persistence.

GPUI 0.2.2 contains native macOS/Linux/Windows platform implementations. Their
presence is not cross-platform execution proof. macOS builds compile a small
AppKit service-provider shim; other targets exercise only the portable handoff.

## Proof commands

Use `just check`, `just test`, and `just build` from this directory.
`--exercise-service-handler --receipt /private/path/receipt.json` opens a native
window, sends a synthetic Unicode selection through the real ObjC handler using
an owned temporary pasteboard on macOS, drains the bounded queue and exits.
Other platforms exercise the portable enqueue path instead.

The callback does not block or replace a queued request. Empty, invalid UTF-8,
NUL-containing, oversized and busy requests fail explicitly. Production input
validation, playback/controller/worker integration and cancellation are separate.

The [2026-10-25 proof](proof.md) executed this native handoff in an ad-hoc
hardened `.app` with strict signature verification and no extra entitlements.
This does NOT establish Services-menu routing from another app, launch-on-demand,
screen-reader accessibility, actual rendering fidelity or Developer ID notarization.
No signing identity, permission or service registration beyond this process is changed.

The eventual app's NSServices declaration belongs in its ordinary bundle
Info.plist, never a launchd agent. Keep weights/private voices/mutable data outside
the sealed bundle. A future packaging proof must sign nested code inside-out,
use hardened runtime/timestamps and require Accepted plus stapled validation.
