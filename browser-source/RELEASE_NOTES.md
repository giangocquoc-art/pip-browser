# Pip 0.1.7

Google DNS diagnostics now test a real Google page through Pip's local DNS-over-HTTPS proxy, rather than checking only whether Google's resolver responds directly. The toolbar and settings distinguish a running route from a saved change that still needs a Pip restart. This setting affects Pip only; it does not change Windows DNS or hide the user's IP address.

Pip can now import and export bookmarks as standard browser HTML, search and remove history entries, and clear cookies plus site storage from the selected profile. These actions are limited to the selected profile and explain which data is preserved.

The Windows installer remains unsigned with Authenticode. GitHub builds the release assets, publishes SHA-256 checksums, and signs artifact provenance; these checks help verify a download but do not remove Windows SmartScreen warnings. Ad and popup filtering remains best-effort and does not guarantee that every ad or redirect is blocked.

# Pip 0.1.6

Broadened gambling pop-up detection for rotating domains: Pip now recognizes common numeric `bet` and `vip` host patterns and gambling terms in popup paths and query strings. These checks apply only to newly opened windows; normal navigation is unchanged. The bundled HaGeZi snapshots remain the primary broad domain coverage, and no filter can guarantee every newly created redirect is caught.

# Pip 0.1.5

Pip now checks popup destinations against HaGeZi's bundled Gambling Mini domain snapshot, in addition to clear gambling-name signals and ad/pop-up lists. Matching the registered domain also catches its subdomains, including newly encountered hostnames. These protections apply only to popup destinations; ordinary page navigation and URLs entered in the address bar remain available. The bundled list is pinned and versioned, so coverage updates with a future Pip release.

The gambling list is embedded as a compact domain index and checked only when a new popup is requested. The app does not download the list or submit browsing URLs to a filtering service.

Ad and popup filtering uses bundled snapshots and conservative domain signals. It does not guarantee that every advertisement or redirect on every website will be blocked.

# Pip 0.1.3

Pip 0.1.3 broadens popup blocking beyond individual domains by adding the pinned HaGeZi Pop-Up Ads list (47,073 rules) to the existing EasyList and uBlock Origin popup filters. While ad blocking is enabled for a site, automatically opened popups are blocked, and user-initiated popup destinations are checked against these lists. The reported `ad88vip6.com` gambling redirect remains in Pip's popup-only overrides. The new list is used only for popups, not when users open a normal link or type a website address.

The source archive includes the Microsoft Store packaging scripts and manifest template, but excludes generated MSIX files and test packages. The browser remains unsigned with Authenticode; GitHub release assets are built by the repository workflow and receive Sigstore artifact attestations after CI passes.

Ad and popup filtering is based on pinned filter snapshots and has limits. It does not guarantee that every advertisement or redirect on every website will be blocked.
