#!/usr/bin/env python3
"""Fetch the Ludusavi manifest and slim it for SaveSync.

Ludusavi's full manifest is ~5MB of YAML covering ~13,000 games. We
only need:
  - the name (the YAML key)
  - the save-path templates per OS
  - the Steam app ID (where present) for matching against Steam's
    libraryfolders.vdf during onboarding
  - the install-dir name (used to resolve <base> paths)

Output: `src-tauri/data/games.json`. Compact JSON, embedded into the
Rust binary at build time via `include_str!`.

Run this script once locally and commit the resulting JSON. Periodic
refresh keeps SaveSync in sync with new game launches; it doesn't
need to be part of CI.

Requires PyYAML (pip install --user --break-system-packages pyyaml).
"""

from __future__ import annotations

import json
import sys
import urllib.request
from pathlib import Path
from typing import Any

try:
    import yaml
except ImportError:
    sys.exit(
        "pyyaml is required. Install with:\n"
        "    pip3 install --user --break-system-packages pyyaml"
    )

MANIFEST_URL = (
    "https://raw.githubusercontent.com/"
    "mtkennerly/ludusavi-manifest/master/data/manifest.yaml"
)

# OSes SaveSync targets. Entries for `mac`, `dos`, etc. are dropped.
RELEVANT_OS = {"windows", "linux", "mac"}


def fetch_yaml(url: str) -> bytes:
    print(f"fetching {url}", file=sys.stderr)
    with urllib.request.urlopen(url, timeout=60) as resp:
        return resp.read()


def slim_game(name: str, entry: dict[str, Any]) -> dict[str, Any] | None:
    """Reduce a single game's entry to the fields SaveSync needs.

    Returns None if the game has no save-tagged paths — we don't track
    games that are just installers or have no persistent state.
    """
    save_paths: list[dict[str, Any]] = []
    for template, meta in (entry.get("files") or {}).items():
        tags = meta.get("tags") or []
        if "save" not in tags:
            continue

        # `when` filters paths by os/store. If absent, the path applies
        # everywhere; we record it as cross-platform.
        whens = meta.get("when") or [{}]
        for when in whens:
            os_name = when.get("os")
            if os_name and os_name not in RELEVANT_OS:
                continue
            save_paths.append(
                {
                    "template": template,
                    "os": os_name,
                    "store": when.get("store"),
                }
            )

    if not save_paths:
        return None

    out: dict[str, Any] = {"save_paths": save_paths}

    steam = entry.get("steam") or {}
    if "id" in steam:
        out["steam_id"] = steam["id"]

    install_dirs = list((entry.get("installDir") or {}).keys())
    if install_dirs:
        out["install_dirs"] = install_dirs

    return out


def main() -> int:
    repo_root = Path(__file__).resolve().parent.parent
    out_dir = repo_root / "src-tauri" / "data"
    out_dir.mkdir(parents=True, exist_ok=True)
    out_path = out_dir / "games.json"

    raw = fetch_yaml(MANIFEST_URL)
    print(f"loading YAML ({len(raw):,} bytes)", file=sys.stderr)
    manifest = yaml.safe_load(raw)
    print(f"parsed {len(manifest):,} games", file=sys.stderr)

    slimmed: dict[str, Any] = {}
    for name, entry in manifest.items():
        slim = slim_game(name, entry or {})
        if slim is not None:
            slimmed[name] = slim

    payload = {
        "schema_version": 1,
        "source": "ludusavi-manifest (https://github.com/mtkennerly/ludusavi-manifest)",
        "games": slimmed,
    }
    out_path.write_text(json.dumps(payload, separators=(",", ":"), sort_keys=True))
    size_kb = out_path.stat().st_size // 1024
    print(
        f"wrote {len(slimmed):,} games with save paths to {out_path} ({size_kb} KB)",
        file=sys.stderr,
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
