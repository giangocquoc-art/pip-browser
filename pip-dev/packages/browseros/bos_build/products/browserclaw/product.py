#!/usr/bin/env python3
"""Pip Dev — the browser for web agents."""

from pathlib import Path

from ...core.products import (
    BROWSEROS_BUG_REPORTER_EXTENSION_ID,
    BROWSERCLAW_EXTENSION_ID,
    MacProductIdentity,
    ProductDescriptor,
    WindowsProductIdentity,
)
from ..server_binaries import ServerBundle, SignSpec

BROWSERCLAW_PRODUCT = ProductDescriptor.define(
    id="browserclaw",
    display_name="Pip Dev",
    windows_installer_guid="{FA2AFFF8-647B-477C-A5D2-905BA8DB9B82}",
    summary="Pip Dev — Chromium browser for developers and AI agents",
    description="Pip Dev is a Chromium-based browser with structured page access for AI agents.",
    homepage_url="https://pip.psharehub.com/",
    support_url="https://pip.psharehub.com/huong-dan/",
    bugtracker_url="https://github.com/giangocquoc-art/pip-browser/issues",
    required_extensions=(
        (BROWSERCLAW_EXTENSION_ID, "Pip Dev app"),
        (BROWSEROS_BUG_REPORTER_EXTENSION_ID, "Pip Dev bug reporter"),
    ),
    server_bundle_ids=("browserclaw-server",),
    artifact_prefix="PipDevSetup",
    mac=MacProductIdentity(
        bundle_id="com.browseros.BrowserClaw",
        dev_bundle_id="com.browseros.dev.BrowserClaw",
        signing_identifier="com.browseros.BrowserClaw",
        dev_signing_identifier="com.browseros.dev.BrowserClaw",
        framework_name="Pip Dev Framework.framework",
        dev_framework_name="Pip Dev Dev Framework.framework",
        dmg_volume_name="Pip Dev",
    ),
    windows=WindowsProductIdentity(
        app_user_model_id="Pip.PipDev",
        installer_app_id="{FA2AFFF8-647B-477C-A5D2-905BA8DB9B82}",
    ),
)

BROWSERCLAW_SERVER_BUNDLE = ServerBundle(
    id="browserclaw-server",
    name="BrowserOS Claw Server",
    product_ids=("browserclaw",),
    chromium_output_root="BrowserClawServer",
    local_resources_root=Path("resources/binaries/browseros_claw_server_rust"),
    chromium_resources_root=Path("chrome/browser/browseros/claw_server/resources"),
    macos_bundle_resources_root=Path(
        "Contents/Resources/BrowserClawServer/default/resources"
    ),
    windows_bundle_resources_root=Path("BrowserClawServer/default/resources"),
    macos_binaries={
        "browseros-claw-server": SignSpec(
            "browseros_claw_server",
            "runtime",
            "browseros-executable-entitlements.plist",
        ),
    },
    windows_binaries=("browseros-claw-server.exe",),
    source_builder="cargo",
    source_component="claw-server-rust",
    runtime_binary_name="browseros-claw-server",
    required_in_chromium_output=False,
    unsigned_artifact_prefix="claw-server-rust/prod-resources",
    unsigned_artifact_base_name="browseros-claw-server-rust-resources",
)
