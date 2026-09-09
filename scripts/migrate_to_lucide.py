#!/usr/bin/env python3
"""Migrate icons under ``assets/icons/`` to Lucide icons."""

import os
import re

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
ICONS_DIR = os.path.join(ROOT, "assets", "icons")
LUCIDE_DIR = "/tmp/lucide_static/package/icons"

MAPPING: dict[str, str] = {
    "activity": "activity",
    "ai-assistant": "sparkles",
    "arrow-up": "arrow-up",
    "ban": "ban",
    "bookmark": "bookmark",
    "brush-cleaning": "brush-cleaning",
    "check": "check",
    "chevron-down": "chevron-down",
    "chevron-left": "chevron-left",
    "chevron-right": "chevron-right",
    "chevron-up": "chevron-up",
    "clipboard-paste": "clipboard-paste",
    "close": "x",
    "cloud": "cloud",
    "cloud-upload": "cloud-upload",
    "code": "code",
    "collapse-dir": "chevrons-down-up",
    "command-action": "square-terminal",
    "copy": "copy",
    "cpu": "cpu",
    "database": "database",
    "download": "download",
    "duplicate": "copy-plus",
    "edit": "pencil",
    "eraser": "eraser",
    "expand-dir": "chevrons-up-down",
    "external-link": "external-link",
    "eye": "eye",
    "eye-off": "eye-off",
    "favorite": "star",
    "file": "file",
    "file-filled": "file-text",
    "focus": "focus",
    "folder": "folder",
    "folder-input": "folder-input",
    "folder-output": "folder-output",
    "folder_open": "folder-open",
    "fullscreen": "maximize",
    "fullscreen-exit": "minimize",
    "git-branch": "git-branch",
    "globe": "globe",
    "hard-drive": "hard-drive",
    "help": "circle-help",
    "image": "image",
    "info": "info",
    "keyboard": "keyboard",
    "layers-2": "layers-2",
    "layout": "layout",
    "link": "link",
    "lock": "lock",
    "memory": "memory-stick",
    "minimize": "minus",
    "monitor": "monitor",
    "monitor-clock": "clock",
    "monitor-users": "users",
    "more_menu": "ellipsis",
    "network": "network",
    "new-dir": "folder-plus",
    "new-file": "file-plus",
    "paint-roller": "paint-roller",
    "panel-attach": "square-arrow-out-down-left",
    "panel-detach": "square-arrow-out-up-right",
    "pause": "pause",
    "play": "play",
    "plus": "plus",
    "quick-command": "zap",
    "refresh": "rotate-cw",
    "save": "save",
    "search": "search",
    "select-all": "check-check",
    "send": "send",
    "serial": "cable",
    "server": "server",
    "settings": "settings",
    "share": "share-2",
    "shield": "shield",
    "sidebar-left": "panel-left",
    "sidebar-right": "panel-right",
    "sort-asc": "arrow-down-narrow-wide",
    "sort-desc": "arrow-down-wide-narrow",
    "split-horizontal": "rows-2",
    "split-vertical": "columns-2",
    "square-activity": "square-activity",
    "stop": "square",
    "telnet": "radio",
    "terminal": "terminal",
    "terminal-minimized": "square-minus",
    "transfer": "arrow-down-up",
    "trash": "trash-2",
    "tunnel": "route",
    "unlink": "unlink",
    "upload": "upload",
    "wrapline": "wrap-text",
}

CUSTOM_ICONS = {
    "tunnel-d",
    "tunnel-l",
    "tunnel-r",
    "window-maximize-win11",
    "window-restore-win11",
}


def normalize_svg(content: str) -> str:
    # Strip XML / HTML comments
    content = re.sub(r"<!--.*?-->", "", content, flags=re.DOTALL)
    # Extract inner elements
    m = re.search(r"<svg[^>]*>(.*)</svg>", content, flags=re.DOTALL)
    if not m:
        raise ValueError("Invalid SVG")
    inner = m.group(1).strip()
    return f'<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" xmlns="http://www.w3.org/2000/svg">\n  {inner}\n</svg>\n'


def main() -> None:
    converted = 0
    skipped = 0
    for dst_name, src_name in sorted(MAPPING.items()):
        src_path = os.path.join(LUCIDE_DIR, f"{src_name}.svg")
        dst_path = os.path.join(ICONS_DIR, f"{dst_name}.svg")
        if not os.path.exists(src_path):
            print(f"Error: missing source {src_path}")
            continue
        with open(src_path, "r", encoding="utf-8") as f:
            raw = f.read()
        normalized = normalize_svg(raw)
        with open(dst_path, "w", encoding="utf-8") as f:
            f.write(normalized)
        converted += 1

    print(f"Successfully converted {converted} icons from Lucide to {ICONS_DIR}")


if __name__ == "__main__":
    main()
