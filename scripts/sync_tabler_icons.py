#!/usr/bin/env python3
"""Sync and normalize Tabler Icons into ``assets/icons/``.

Fetches standard outline SVGs from Tabler Icons CDN/repository, normalizes them
to Velowork's design contract (viewBox="0 0 24 24", fill="none", stroke="currentColor",
stroke-width="2", no hard-coded width/height/class), and writes them to ``assets/icons/``.

Usage:
    python3 scripts/sync_tabler_icons.py
"""

from __future__ import annotations

import os
import re
import urllib.error
import urllib.request

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
ICONS_DIR = os.path.join(ROOT, "assets", "icons")

# Primary CDN and fallbacks for Tabler Icons SVGs
CDN_URLS = [
    "https://cdn.jsdelivr.net/npm/@tabler/icons@latest/icons/outline/{name}.svg",
    "https://raw.githubusercontent.com/tabler/tabler-icons/main/icons/outline/{name}.svg",
    "https://unpkg.com/@tabler/icons@latest/icons/outline/{name}.svg",
]

# Mapping: local_filename_stem -> tabler_icon_name
ICON_MAPPING: dict[str, str] = {
    # Chevrons & Direction
    "arrow-up": "arrow-up",
    "chevron-down": "chevron-down",
    "chevron-left": "chevron-left",
    "chevron-right": "chevron-right",
    "chevron-up": "chevron-up",
    "dropdown": "caret-down",

    # Actions & Editing
    "check": "check",
    "close": "x",
    "copy": "copy",
    "delete": "trash",
    "duplicate": "files",
    "edit": "pencil",
    "eraser": "eraser",
    "export": "file-export",
    "import": "file-import",
    "plus": "plus",
    "recall": "history",
    "refresh": "refresh",
    "save": "device-floppy",
    "search": "search",
    "select-all": "select-all",
    "send": "send",
    "trash": "trash",

    # Files & Folders
    "clipboard-paste": "clipboard",
    "collapse-dir": "fold",
    "expand-dir": "fold-down",
    "file": "file",
    "file-copy": "file-plus",
    "file-filled": "file-text",
    "folder": "folder",
    "folder_open": "folder-open",
    "new-dir": "folder-plus",
    "new-file": "file-plus",

    # State & Feedback
    "activity": "activity",
    "ban": "ban",
    "bell": "bell",
    "bookmark": "bookmark",
    "favorite": "star",
    "favorite-off": "star-off",
    "help": "help",
    "lightbulb": "bulb",
    "lock": "lock",
    "shield": "shield",
    "star": "star",
    "eye": "eye",
    "eye-off": "eye-off",

    # Links & Network
    "external-link": "external-link",
    "link": "link",
    "network": "network",
    "unlink": "unlink",

    # UI & Layout
    "box": "box",
    "collapse": "fold",
    "focus": "focus-2",
    "fullscreen": "maximize",
    "fullscreen-exit": "minimize",
    "layout": "layout",
    "minimize": "minus",
    "more_menu": "dots",
    "sidebar-left": "layout-sidebar",
    "sidebar-right": "layout-sidebar-right",
    "sort-asc": "sort-ascending",
    "sort-desc": "sort-descending",
    "split-horizontal": "layout-rows",
    "split-vertical": "layout-columns",
    "tabs": "app-window",
    "unfold-vertical": "arrows-split-2",
    "wrapline": "text-wrap",

    # Git
    "git-branch": "git-branch",
    "git-commit": "git-commit",
    "git-pull-request": "git-pull-request",

    # System, Hardware & Dev
    "cloud": "cloud",
    "code": "code",
    "command": "command",
    "cpu": "cpu",
    "database": "database",
    "globe": "world",
    "image": "photo",
    "keyboard": "keyboard",
    "log-out": "logout",
    "memory": "cpu-2",
    "monitor": "device-desktop",
    "pause": "player-pause",
    "play": "player-play",
    "settings": "settings",
    "share": "share",
    "stop": "player-stop",
    "system": "adjustments",
    "terminal": "terminal-2",
    "terminal-env": "terminal",
    "terminal-minimized": "prompt",
}


def fetch_tabler_svg(tabler_name: str) -> str:
    """Download SVG content for tabler_name from CDN/repo."""
    headers = {"User-Agent": "Mozilla/5.0 (compatible; VeloworkIconSync/1.0)"}
    for template in CDN_URLS:
        url = template.format(name=tabler_name)
        try:
            req = urllib.request.Request(url, headers=headers)
            with urllib.request.urlopen(req, timeout=10) as resp:
                if resp.status == 200:
                    return resp.read().decode("utf-8")
        except Exception:
            continue
    raise RuntimeError(f"Failed to download Tabler icon: {tabler_name}")


def normalize_svg(raw_svg: str) -> str:
    """Normalize SVG to Velowork's design contract.

    Removes width, height, and class attributes on the root <svg> element.
    Ensures viewBox="0 0 24 24", fill="none", stroke="currentColor", stroke-width="2",
    stroke-linecap="round", stroke-linejoin="round".
    """
    svg = raw_svg.strip()

    # Find the opening <svg ...> tag
    match = re.match(r"^<svg\b([^>]*)>(.*)</svg>$", svg, re.DOTALL | re.IGNORECASE)
    if not match:
        raise ValueError("Invalid SVG structure")

    attrs_str, inner_content = match.groups()

    # Normalize inner content: strip whitespace
    inner = inner_content.strip()

    # Construct standard normalized <svg>
    normalized = (
        '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" '
        'stroke-linecap="round" stroke-linejoin="round" xmlns="http://www.w3.org/2000/svg">\n'
    )
    # Indent inner lines by 2 spaces if not already indented
    for line in inner.splitlines():
        line = line.strip()
        if line:
            normalized += f"  {line}\n"
    normalized += "</svg>\n"

    return normalized


def sync_all() -> None:
    os.makedirs(ICONS_DIR, exist_ok=True)
    total = len(ICON_MAPPING)
    success = 0
    failed = []

    print(f"Syncing {total} icons to Tabler Icons...")

    for idx, (local_name, tabler_name) in enumerate(sorted(ICON_MAPPING.items()), 1):
        target_path = os.path.join(ICONS_DIR, f"{local_name}.svg")
        try:
            raw_svg = fetch_tabler_svg(tabler_name)
            clean_svg = normalize_svg(raw_svg)
            with open(target_path, "w", encoding="utf-8") as f:
                f.write(clean_svg)
            success += 1
            print(f"[{idx}/{total}] OK: {local_name}.svg <- tabler:{tabler_name}")
        except Exception as e:
            failed.append((local_name, tabler_name, str(e)))
            print(f"[{idx}/{total}] FAIL: {local_name}.svg <- tabler:{tabler_name}: {e}")

    print(f"\nCompleted: {success}/{total} updated successfully.")
    if failed:
        print(f"Failed icons ({len(failed)}):")
        for local_name, tabler_name, err in failed:
            print(f"  - {local_name} (tabler:{tabler_name}): {err}")
        raise SystemExit(1)


if __name__ == "__main__":
    sync_all()
