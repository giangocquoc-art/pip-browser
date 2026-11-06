# Pip Microsoft Store package

Pip is a Win32/WebView2 application, so the Store distribution path that includes Microsoft-managed signing is MSIX. Do not submit `PipSetup.exe` as the Store app package: Microsoft does not re-sign EXE/MSI submissions.

## Before packaging

1. Start at [Microsoft Store Developer](https://storedeveloper.microsoft.com/) using the personal Microsoft account that should own Pip. As of 2026-10-18, Microsoft's [new individual account flow](https://learn.microsoft.com/en-us/windows/apps/publish/whats-new-individual-developer) waives the registration fee in nearly 200 markets. Start from the Store Developer page; Microsoft says direct Partner Center entry can show the legacy flow. The account owner must complete identity verification using their government-issued ID and selfie.
2. After verification, open Partner Center, create a new MSIX product, and reserve the name `Pip` (or the closest available name).
3. Open Product identity and copy the exact Package/Identity/Name and Publisher values. The publisher display name is also part of the public Store identity.
4. Build the current browser binary with `cargo build -p evergreen-browser --release --locked`.

The truthful Vietnamese/English Store copy, technical details, privacy/support links, and certification notes are in [STORE_LISTING_DRAFT.md](STORE_LISTING_DRAFT.md). Verify it against the exact build before submitting.

## Build the package

From the `Pip` repository root, run:

```powershell
.\store\package-store-msix.ps1 `
  -IdentityName 'PARTNER_CENTER_PACKAGE_NAME' `
  -PublisherDistinguishedName 'PARTNER_CENTER_PUBLISHER' `
  -PublisherDisplayName 'YOUR_STORE_PUBLISHER_NAME'
```

The script creates `store/out/Pip_<version>_x64.msix` and a Partner Center `.msixupload` archive, then unpacks the MSIX and verifies that the packaged executable is byte-for-byte identical to the selected build. `store/out` is ignored by Git. Known placeholder identity values are rejected unless `-AllowTestIdentity` is passed for CI or local preflight; those test packages receive adjacent `DO_NOT_SUBMIT` marker files. Microsoft signs the Store release after certification.

## Runtime and certification notes

- Pip requires the Microsoft Edge WebView2 Runtime. Windows 11 includes Evergreen WebView2; many Windows 10 PCs already have it, but a small number do not. If the runtime is absent, Pip now explains the requirement and offers to open Microsoft's official download page. Test this recovery path on a clean supported Windows 10 image before broad Store release.
- The Store package omits `portable.ini`, so the app uses its normal per-user AppData storage rather than trying to write under the read-only package install directory.
- The app is a full-trust desktop process and declares `runFullTrust`. Run Windows App Certification Kit and real install/upgrade/uninstall tests after creating the actual Partner Center identity.
- The Store manages app installation, signing, and updates. Do not distribute this unsigned MSIX directly from the Pip website. The existing `PipSetup.exe` route remains separate and still needs a public code-signing certificate to improve its publisher reputation.
