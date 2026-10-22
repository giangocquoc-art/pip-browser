# Pip 0.1.16

Google DNS now falls back to Windows DNS only when both Google A and AAAA lookups fail transiently. Pip reports when this fallback is used; NXDOMAIN, NODATA, invalid DNS responses, and proxy or HTTPS failures do not trigger an unannounced direct route. Updated the privacy notice and Google DNS guide to explain that Windows DNS may see a hostname during fallback. Fixed the proxy HTTP request-line terminator and kept background requests on the configured proxy when Google DNS is enabled.

# Pip 0.1.15

Updated TLS certificate verification and bundled root certificates to patched versions after new RustSec advisories. This release also includes the popup-filtering improvements from the unpublished 0.1.14 candidate.

# Pip 0.1.14

Fixed a production gap in popup filtering: the bundled popup-domain list is now applied to matching popup, redirect, and frame destinations, including subdomains. This affects newly opened destinations only; ordinary navigation and addresses entered by the user remain available. Ad and popup filtering remains best-effort.

# Pip 0.1.13

Fixed Google DNS routing for WebView2 by forcing web traffic through Pip's local DNS-over-HTTPS proxy and preventing direct system-DNS lookups. Tightened proxy URL and DNS-answer parsing, respected DNS record TTLs, and verified that YouTube loads through the proxy without showing its offline page.

Expanded optional Pip Sync beyond bookmarks: history, open tabs, selected settings, and the Pip password vault can each be enabled separately. These categories are off by default, and enabling password sync requires a separate confirmation. Sync data is encrypted on the device before it is sent to Cloudflare; cookies, website data, WebView2 passwords, and payment details are excluded.

# Pip 0.1.12

Added a Keep Awake option in the Pip menu. It protects the selected tab from automatic suspension, displays its state, and persists the choice with the tab session; legacy sessions default to normal sleeping behavior. Audio, downloads, pinned tabs, and user-protected tabs stay active. Added a guided video playback repair action in the Pip menu. After confirmation, Pip disables ad blocking for the current website only and reloads that page; the choice is saved until the user turns blocking back on. The prompt warns that unsent page content may be lost. Google DNS proxy handling now switches accepted Windows sockets to blocking mode before relaying traffic, so WebView2 HTTPS requests can pass through the local DoH proxy.

# Pip 0.1.11

Added persistent tab groups, on-device page translation where supported by WebView2, and optional encrypted bookmark sync across devices. Pip Sync encrypts bookmark data on the device, protects local credentials with Windows DPAPI, merges deletion markers, and uses revision checks to prevent stale writes. History, passwords, cookies, and sessions are not synced. The sync endpoint is deployed on Cloudflare and passed live HTTPS authentication, read/write, conflict, and deletion checks.

Google DNS remains a Pip-only HTTPS proxy and does not change Windows DNS. The Windows installer remains unsigned with Authenticode; SHA-256 and build attestations do not remove SmartScreen warnings.

# Pip 0.1.10

Google DNS now runs a startup check through the same local HTTPS proxy used by Pip's tabs. The toolbar and settings distinguish an open proxy from a verified route, show an actionable failure state, and never claim the route works before a real page loads. When a saved DNS change needs a restart, the toolbar button now changes to “Áp dụng” and can restart Pip directly after confirmation. The check only affects Pip; Windows DNS is unchanged.

The website adds a trust and verification guide for official downloads, SHA-256, GitHub build attestations, and the current Authenticode signing status. Website CI now checks local links, sitemap routes, the download button, and required trust disclosures.

The Windows installer remains unsigned with Authenticode. GitHub build attestations and SHA-256 checks help verify release origin and integrity but do not remove SmartScreen warnings. Pip is preparing Microsoft Store distribution, which requires the account owner's identity verification and app certification.

# Pip 0.1.9

Google DNS settings now show whether the route is active and provide a one-click restart that saves and restores regular tabs and profile windows. Private windows close during restart. The DNS proxy disables QUIC so supported web traffic uses the configured HTTPS route. Live DNS-over-HTTPS and proxy route checks passed against Google.

# Pip 0.1.8

Updated Pip's bundled HaGeZi Gambling Mini popup destination list to pinned upstream version 2026.1017.1606.06. The refreshed snapshot adds 234 domains and removes 20 stale entries. These matches apply to popup destinations only; normal page navigation is unchanged. The bundled list works offline and is not sent any browsing URLs.

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
