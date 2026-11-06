"""Capture offline UI templates with Edge headless without opening a window."""
import os
from pathlib import Path
import subprocess
import tempfile

root = Path(__file__).resolve().parents[1]
review = root / "ui-review"
edge = Path(os.environ["ProgramFiles(x86)"]) / "Microsoft/Edge/Application/msedge.exe"
if not edge.is_file():
    raise SystemExit("Microsoft Edge was not found at the standard installation path")
profile = Path(tempfile.mkdtemp(prefix="pip-ui-review-"))


def capture(page: str, name: str, size: tuple[int, int], dark: bool = False) -> None:
    width, height = size
    args = [
        str(edge),
        "--headless=new",
        "--disable-gpu",
        "--disable-extensions",
        "--disable-background-networking",
        "--disable-component-update",
        "--hide-scrollbars",
        "--no-first-run",
        "--no-default-browser-check",
        f"--user-data-dir={profile}",
        f"--window-size={width},{height}",
        f"--screenshot={review / name}",
    ]
    if dark:
        args.append("--force-dark-mode")
    args.append((review / page).as_uri())
    flags = subprocess.CREATE_NO_WINDOW if os.name == "nt" else 0
    result = subprocess.run(args, cwd=review, capture_output=True, timeout=35, creationflags=flags)
    image = review / name
    if result.returncode or not image.is_file() or image.stat().st_size < 1024:
        raise RuntimeError(f"Edge failed to render {page}: {result.stderr.decode(errors='replace')}")
    print(f"{name}: {image.stat().st_size} bytes")


capture("installer.html", "installer.png", (580, 500))
capture("settings.html", "settings-desktop.png", (1280, 800))
capture("settings.html", "settings-narrow.png", (640, 800))
capture("sidebar.html", "sidebar.png", (320, 760))
capture("home.html", "home.png", (1280, 800))
capture("error.html", "error.png", (1280, 800))
capture("settings.html", "settings-dark.png", (1280, 800), dark=True)
capture("home.html", "home-dark.png", (1280, 800), dark=True)
capture("chrome-profile-menu.html", "chrome-profile-menu.png", (1280, 440))
capture("chrome-profile-menu.html", "chrome-profile-menu-dark.png", (1280, 440), dark=True)
print(f"Rendered evidence: {review}")
