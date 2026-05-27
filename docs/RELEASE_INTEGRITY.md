# Release Integrity

raria release archives are built by GitHub Actions from repository source and
published with SHA-256 checksum files.

## Artifacts

Expected release archive names follow the target triple:

```text
raria-x86_64-unknown-linux-gnu.tar.gz
raria-aarch64-unknown-linux-gnu.tar.gz
raria-x86_64-apple-darwin.tar.gz
raria-aarch64-apple-darwin.tar.gz
raria-x86_64-pc-windows-msvc.zip
raria-aarch64-pc-windows-msvc.zip
```

Each archive should have a matching `.sha256` file in the same GitHub Release.

## Verify Checksums

macOS and Linux:

```bash
shasum -a 256 -c raria-aarch64-apple-darwin.tar.gz.sha256
```

Linux with GNU coreutils:

```bash
sha256sum -c raria-x86_64-unknown-linux-gnu.tar.gz.sha256
```

Windows PowerShell:

```powershell
Get-FileHash .\raria-x86_64-pc-windows-msvc.zip -Algorithm SHA256
Get-Content .\raria-x86_64-pc-windows-msvc.zip.sha256
```

The computed digest must match the digest in the `.sha256` file.

## Signing Status

The current release contract is checksummed standalone CLI archives. Code
signing, notarized installers, package-manager formulas, and auto-update
metadata are not part of the current raria release pipeline.
