#!/usr/bin/env python3
"""Compile a String Catalog (.xcstrings) into <lang>.lproj/<table>.strings files.

This stands in for `xcstringstool`, which only ships with Xcode. It handles
plain string units, which is all the catalogs in this project use. Plural or
device variations and substitutions are rejected with an error, so a new
catalog feature cannot be dropped silently. Keys without a translation for a
language are left out, so lookups fall back to the key, as in the Xcode build.

Usage: xcstrings2strings.py <catalog.xcstrings> <output Resources dir>
"""

from __future__ import annotations

import json
import plistlib
import sys
from pathlib import Path


def compile_catalog(catalog_path: Path, out_dir: Path) -> None:
    catalog = json.loads(catalog_path.read_text(encoding="utf-8"))
    table = catalog_path.stem  # the table name is the catalog's file name
    tables: dict[str, dict[str, str]] = {}

    for key, entry in catalog.get("strings", {}).items():
        if entry.get("shouldTranslate") is False:
            continue
        for lang, loc in entry.get("localizations", {}).items():
            if "variations" in loc or "substitutions" in loc:
                sys.exit(f"{catalog_path}: '{key}' ({lang}) uses variations or substitutions, "
                         "which this script does not support; build with Xcode or extend the script")
            value = loc.get("stringUnit", {}).get("value")
            if value is not None:
                tables.setdefault(lang, {})[key] = value

    for lang, strings in sorted(tables.items()):
        lproj = out_dir / f"{lang}.lproj"
        lproj.mkdir(parents=True, exist_ok=True)
        # Xcode also writes compiled .strings files as binary property lists.
        with open(lproj / f"{table}.strings", "wb") as f:
            plistlib.dump(strings, f, fmt=plistlib.FMT_BINARY, sort_keys=True)


def main() -> None:
    if len(sys.argv) != 3:
        sys.exit(__doc__)
    compile_catalog(Path(sys.argv[1]), Path(sys.argv[2]))


if __name__ == "__main__":
    main()
