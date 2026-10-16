# Browser source snapshot

This directory tracks the Pip Browser 0.1.3 source used to prepare the next verified release. The release workflow builds the Windows installer from this snapshot, runs Windows CI, publishes a SHA-256 manifest, and creates Sigstore artifact attestations. After validation, the versioned source ZIP and its checksum are attached to the matching GitHub Release.

See `PIP_README.md`, `RELEASE_NOTES.md`, `THIRD_PARTY.md`, and `licenses/` for product guidance and dependency notices. Generated installers, executables, user data, and Store test packages are excluded from the source archive.
