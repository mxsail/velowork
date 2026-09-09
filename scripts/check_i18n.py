#!/usr/bin/env python3
import os
import re
import json
import sys
from pathlib import Path

def flatten_json(data, prefix=""):
    items = {}
    if isinstance(data, dict):
        for k, v in data.items():
            new_key = f"{prefix}.{k}" if prefix else k
            if isinstance(v, dict):
                items.update(flatten_json(v, new_key))
            elif isinstance(v, str):
                items[new_key] = v
    return items

def main():
    repo_root = Path("/home/choi/Workspaces/velowork")
    zh_path = repo_root / "crates/velowork-i18n/locales/zh.json"
    en_path = repo_root / "crates/velowork-i18n/locales/en.json"

    with open(zh_path, "r", encoding="utf-8") as f:
        zh_data = json.load(f)
    with open(en_path, "r", encoding="utf-8") as f:
        en_data = json.load(f)

    zh_keys = flatten_json(zh_data)
    en_keys = flatten_json(en_data)

    print(f"Loaded {len(zh_keys)} keys from zh.json")
    print(f"Loaded {len(en_keys)} keys from en.json")

    # 1. Symmetry check between en and zh
    zh_only = set(zh_keys.keys()) - set(en_keys.keys())
    en_only = set(en_keys.keys()) - set(zh_keys.keys())

    if zh_only:
        print(f"\n[WARN] {len(zh_only)} keys only in zh.json:")
        for k in sorted(zh_only):
            print(f"  + {k}")
    if en_only:
        print(f"\n[WARN] {len(en_only)} keys only in en.json:")
        for k in sorted(en_only):
            print(f"  + {k}")

    # 2. Extract i18n keys from all .rs files
    patterns = [
        re.compile(r'i18n!\s*\([^,]+,\s*"([^"]+)"\s*\)'),
        re.compile(r't_fmt\s*\([^,]+,\s*"([^"]+)"'),
        re.compile(r't_cx\s*\([^,]+,\s*"([^"]+)"'),
        re.compile(r'velowork_i18n::t\s*\([^,]+,\s*"([^"]+)"'),
        re.compile(r'velowork_i18n::t_fmt\s*\([^,]+,\s*"([^"]+)"'),
    ]

    used_keys = {} # key -> list of (file, line_no)

    for root, _, files in os.walk(repo_root):
        if "target" in root or ".git" in root:
            continue
        for file in files:
            if file.endswith(".rs"):
                filepath = Path(root) / file
                rel_path = filepath.relative_to(repo_root)
                with open(filepath, "r", encoding="utf-8", errors="ignore") as f:
                    for line_no, line in enumerate(f, 1):
                        for pattern in patterns:
                            for match in pattern.finditer(line):
                                k = match.group(1)
                                if k not in used_keys:
                                    used_keys[k] = []
                                used_keys[k].append((str(rel_path), line_no))

    print(f"\nExtracted {len(used_keys)} unique i18n key references from Rust files.")

    # 2.1 Scan descriptions.rs for dynamic commands.<name> and commands.<desc>
    desc_file = repo_root / "crates/velowork-app/src/keybindings/descriptions.rs"
    if desc_file.exists():
        with open(desc_file, "r", encoding="utf-8") as f:
            desc_text = f.read()
        pattern_desc = re.compile(r'ActionDescription\s*\{\s*name:\s*"([^"]+)",\s*description:\s*"([^"]+)"')
        for match in pattern_desc.finditer(desc_text):
            name_k = f"commands.{match.group(1)}"
            desc_k = f"commands.{match.group(2)}"
            rel_desc = "crates/velowork-app/src/keybindings/descriptions.rs"
            used_keys.setdefault(name_k, []).append((rel_desc, 0))
            if match.group(2).strip():
                used_keys.setdefault(desc_k, []).append((rel_desc, 0))

    # 3. Check for missing keys
    missing_in_zh = {}
    missing_in_en = {}

    for k, locations in used_keys.items():
        if k not in zh_keys:
            missing_in_zh[k] = locations
        if k not in en_keys:
            missing_in_en[k] = locations

    print("\n=======================================================")
    print(f"MISSING KEYS IN zh.json: {len(missing_in_zh)}")
    print("=======================================================")
    for k, locs in sorted(missing_in_zh.items()):
        print(f"\nKey: \"{k}\"")
        for f, line in locs[:5]:
            print(f"  at {f}:{line}")

    print("\n=======================================================")
    print(f"MISSING KEYS IN en.json: {len(missing_in_en)}")
    print("=======================================================")
    for k, locs in sorted(missing_in_en.items()):
        print(f"\nKey: \"{k}\"")
        for f, line in locs[:5]:
            print(f"  at {f}:{line}")

    # Check for placeholder mismatches
    print("\n=======================================================")
    print("CHECKING PLACEHOLDER MATCHES")
    print("=======================================================")
    placeholder_pattern = re.compile(r'\{([a-zA-Z0-9_]+)\}')
    for k in set(zh_keys.keys()) & set(en_keys.keys()):
        zh_val = zh_keys[k]
        en_val = en_keys[k]
        zh_ph = set(placeholder_pattern.findall(zh_val))
        en_ph = set(placeholder_pattern.findall(en_val))
        if zh_ph != en_ph:
            print(f"Placeholder mismatch for '{k}': zh={zh_ph} (\"{zh_val}\"), en={en_ph} (\"{en_val}\")")

    # 4. Check for unreferenced/dead keys in JSON
    all_json_keys = set(zh_keys.keys()) | set(en_keys.keys())
    # Note: some keys might be dynamically generated like `desc_key` or `level_key` or prefixed.
    # We can detect directly unused keys.
    print(f"\nTotal keys in locale files: {len(all_json_keys)}")
    print(f"Directly matched keys in Rust code: {len(set(used_keys.keys()) & all_json_keys)}")

if __name__ == "__main__":
    main()
