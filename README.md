# Pip website

A lightweight Vietnamese download page for Pip Browser. It uses plain HTML, CSS, JavaScript, and two local WebP illustrations; there is no build step, framework, web font, or animation. The browser, menu, and settings are labeled visual previews.

`version.json` is the release marker for the included installer. Update it with each published installer’s version, URL, SHA-256, size, and date; the page enables the download button and announces a newer version. The setup file is included in this repository for Vercel static hosting. The installer is Windows x64, per-user, unsigned, and passed silent install/uninstall checks in a disposable profile on 2026-10-16.

The built-in ad filter runs in the background; filtering effectiveness depends on the website. Mascot art follows the supplied PShareHub reference.
