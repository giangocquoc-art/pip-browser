#!/usr/bin/env python3
"""Export Pip bookmarks/history/profile labels for a future Pip Dev importer.

The export intentionally excludes cookies, credentials, and saved passwords.
"""

from __future__ import annotations

import argparse
import json
import os
import re
from datetime import datetime, timezone
from pathlib import Path


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--data-dir",
        type=Path,
        default=Path(os.environ.get("APPDATA", Path.home())) / "evergreen-browser",
        help="Pip data directory (default: %%APPDATA%%/evergreen-browser)",
    )
    parser.add_argument("--output", type=Path, required=True, help="Destination JSON file")
    args = parser.parse_args()

    settings_path = args.data_dir / "settings.json"
    if not settings_path.is_file():
        parser.error(f"Pip settings not found: {settings_path}")
    output_path = args.output.resolve()
    if output_path.exists():
        parser.error(f"Refusing to overwrite an existing file: {output_path}")
    settings = json.loads(settings_path.read_text(encoding="utf-8"))
    profiles = []
    for profile in settings.get("profiles", []):
        profile_id = profile.get("id")
        if not isinstance(profile_id, str) or not re.fullmatch(r"[A-Za-z0-9_-]{1,128}", profile_id):
            continue
        library_path = args.data_dir / "profiles" / profile_id / "library.json"
        library = json.loads(library_path.read_text(encoding="utf-8")) if library_path.is_file() else {}
        profiles.append(
            {
                "id": profile_id,
                "name": profile.get("name", profile_id),
                "bookmarks": library.get("bookmarks", []),
                "bookmark_folders": library.get("bookmark_folders", []),
                "bookmark_folder_assignments": library.get("bookmark_folder_assignments", []),
                "history": library.get("history", []),
                "history_visits": library.get("history_visits", []),
            }
        )

    bundle = {
        "format": "pip-dev-migration",
        "version": 1,
        "exported_at": datetime.now(timezone.utc).isoformat(),
        "source": "Pip",
        "profiles": profiles,
        "settings": {
            "search_engine": settings.get("search_engine", "google"),
            "appearance": settings.get("appearance", {}),
            "performance": settings.get("performance", {}),
            "privacy": settings.get("privacy", {}),
            "tabs": settings.get("tabs", {}),
            "adblock_mode": settings.get("adblock_mode"),
            "google_dns": settings.get("google_dns", False),
        },
        "excluded": ["cookies", "saved passwords", "site login sessions", "autofill secrets"],
    }
    output_path.parent.mkdir(parents=True, exist_ok=True)
    output_path.write_text(json.dumps(bundle, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    print(f"Exported {len(profiles)} profile(s) to {output_path}")
    print("Cookies and credentials were excluded; sign in to websites again after migration.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
