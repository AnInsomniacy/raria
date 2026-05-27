# Code Signing Status

raria currently publishes checksummed standalone CLI archives. The release
pipeline does not produce signed installers, notarized macOS apps, Windows
Authenticode signatures, package-manager formulas, or auto-update metadata.

## Current Integrity Boundary

Official release assets are built by GitHub Actions and uploaded to a GitHub
Release with matching SHA-256 checksum files. Users and integrators should
verify the archive checksum before installing or redistributing a binary.

See [Release Integrity](RELEASE_INTEGRITY.md) for verification commands.

## Future Signing

Code signing can be added later if the project adopts installer packages,
package-manager distribution, or auto-update channels. Any signing change must
document the trust boundary, signing identity, CI workflow, key ownership,
artifact names, and recovery process before the first signed release.
