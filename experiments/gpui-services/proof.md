# macOS probe execution boundary

## Observed 2026-10-25

Probe native source: main `008a259864df58f752e1740b9791d45594fc0834`. `just check test build` passed; four portable handoff tests passed. Existing dependency future-incompatibility warnings remain; this is not a warning-free future-Rust claim.

A newly staged ordinary `.app` with one debug-profile GPUI probe executable was signed **ad hoc**, hardened runtime enabled, no added entitlements. `codesign --verify --strict` passed and signature metadata reported `adhoc,runtime`. Direct execution of that bundle's executable with `--exercise-service-handler --receipt …` returned zero, produced a GPUI application-loop receipt and accepted exactly one synthetic Unicode selection through the real Objective-C callback and owned temporary pasteboard. The process exited on its own.

Signed executable SHA256: `9d41c03505111d7c5e2515b6fc16c360d1d1aae34f8b7f0bf9e708ed714b0256`.

No real selection, general clipboard, microphone, Accessibility or screen-capture permission was requested. No explicit global Services registration command, signing-identity inspection, certificate installation, notarization upload or launchd operation was performed. Normal application execution is not a bitwise audit of OS caches.

## What this proves — and does not

This establishes that the isolated GPUI/ObjC handoff probe executes in an ad-hoc hardened bundle without extra entitlements. It does **not** establish Developer ID trust, notarization, stapling, Gatekeeper distribution, actual Services-menu/launch-on-demand routing, a signed MLX worker/metallib bundle, accessibility, selected-text capture from arbitrary apps or Linux/Windows execution.

The test bundle intentionally has no `NSServices` declaration; the callback is invoked in process. A production service declaration and independent client-routing test remain necessary. No approved product UI was built or captured.

## Reproduce the same bounded proof

1. Run `just check test build` here. Obtain the executable from Cargo's `--message-format=json` compiler-artifact record, not an mtime guess.
2. Create a fresh private output directory and `SPQXServicesProbe.app/Contents/MacOS/`. Copy only that executable as `spqx-gui-services-probe`.
3. Write its ordinary bundle `Contents/Info.plist` with `CFBundleExecutable=spqx-gui-services-probe`, `CFBundleIdentifier=dev.byteowlz.spqx.services-probe`, `CFBundleName=SPQXServicesProbe`, `CFBundlePackageType=APPL`, bundle version1, short version0.0.0, `NSHighResolutionCapable=true` and `NSPrincipalClass=NSApplication`. Do not create a launchd plist or add an `NSServices` registration for this in-process exercise.
4. Sign only this owned test bundle, with no credential-backed identity or added entitlements:

   ```sh
   codesign --force --sign - --options runtime "$APP"
   codesign --verify --strict --verbose=2 "$APP"
   codesign -d --verbose=4 "$APP"
   "$APP/Contents/MacOS/spqx-gui-services-probe" \
     --exercise-service-handler --receipt "$PRIVATE_OUTPUT/receipt.json"
   ```

5. Bound execution to30 seconds; terminate only the owned child on timeout. Require process exit0 and receipt `platform=macos`, `gpui_application_loop=true`, `accepted=1`, `os_services_routing_tested=false`, `inference=false`. Record signed binary SHA256, native source hashes and signature output privately. Failure is not permission to add broad signing exceptions.
