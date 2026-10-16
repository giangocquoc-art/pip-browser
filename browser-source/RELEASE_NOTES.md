# Pip 0.1.4

Pip now blocks popup destinations whose host clearly identifies common gambling categories (casino, betting, slots, togel, tài xỉu, xóc đĩa, and related terms). This catches some newly rotated gambling domains even when WebView2 reports that an ad redirect followed a click. The rule is popup-only; ordinary navigation and URLs typed into the address bar are unaffected. EasyList, uBlock Origin, and HaGeZi popup rules continue to cover other known destinations.

Ad and popup filtering uses bundled snapshots and conservative domain signals. It does not guarantee that every advertisement or redirect on every website will be blocked.

# Pip 0.1.3

Pip 0.1.3 broadens popup blocking beyond individual domains by adding the pinned HaGeZi Pop-Up Ads list (47,073 rules) to the existing EasyList and uBlock Origin popup filters. While ad blocking is enabled for a site, automatically opened popups are blocked, and user-initiated popup destinations are checked against these lists. The reported `ad88vip6.com` gambling redirect remains in Pip's popup-only overrides. The new list is used only for popups, not when users open a normal link or type a website address.

The source archive includes the Microsoft Store packaging scripts and manifest template, but excludes generated MSIX files and test packages. The browser remains unsigned with Authenticode; GitHub release assets are built by the repository workflow and receive Sigstore artifact attestations after CI passes.

Ad and popup filtering is based on pinned filter snapshots and has limits. It does not guarantee that every advertisement or redirect on every website will be blocked.
