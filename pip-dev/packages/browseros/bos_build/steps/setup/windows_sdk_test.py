#!/usr/bin/env python3
"""Tests for the pinned Windows SDK and toolchain prerequisites."""

import tempfile
import unittest
from pathlib import Path

from .windows_sdk import _atl_header


class AtlHeaderTest(unittest.TestCase):
    def test_finds_atl_in_current_msvc_layout(self):
        with tempfile.TemporaryDirectory() as tmp:
            header = (
                Path(tmp)
                / "VC"
                / "Tools"
                / "MSVC"
                / "14.44.35207"
                / "atlmfc"
                / "include"
                / "atldef.h"
            )
            header.parent.mkdir(parents=True)
            header.write_text("// ATL header\n", encoding="utf-8")

            self.assertEqual(_atl_header(tmp), header)

    def test_finds_atl_in_legacy_layout(self):
        with tempfile.TemporaryDirectory() as tmp:
            header = Path(tmp) / "VC" / "atlmfc" / "include" / "atldef.h"
            header.parent.mkdir(parents=True)
            header.write_text("// ATL header\n", encoding="utf-8")

            self.assertEqual(_atl_header(tmp), header)

    def test_returns_none_when_atl_is_missing(self):
        with tempfile.TemporaryDirectory() as tmp:
            self.assertIsNone(_atl_header(tmp))
