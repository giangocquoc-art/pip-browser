# Pip 0.1.2

Pip 0.1.2 combines the native title bar and tab strip into one browser row and broadens popup protection. While ad blocking is enabled for a site, automatically opened popups are blocked; recognized popup and popunder rules from the bundled EasyList and uBlock Origin filters also apply to user-initiated popup destinations. The reported `ad88vip6.com` gambling redirect is included in Pip's popup-only overrides.

The source archive now includes the Microsoft Store packaging scripts and manifest template, but excludes generated MSIX files and test packages. The browser remains unsigned with Authenticode; GitHub release assets are built by the repository workflow and receive Sigstore artifact attestations after CI passes.

Ad and popup filtering is based on pinned filter snapshots and has limits. It does not guarantee that every advertisement or redirect on every website will be blocked.
