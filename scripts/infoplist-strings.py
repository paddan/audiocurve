#!/usr/bin/env python3
"""Write <lang>.lproj/InfoPlist.strings for the user-visible Info.plist keys.

The English text is the value in the Info.plist itself; its translations are looked up in a
String Catalog (.xcstrings), with that English text as the key. Every language the catalog
has for a key must translate it, so a translation cannot go missing silently.

Usage: infoplist-strings.py <Info.plist> <catalog.xcstrings> <output Resources dir>
"""

from __future__ import annotations

import json
import plistlib
import sys
from pathlib import Path

# The Info.plist keys macOS shows to the user.
KEYS = ["NSAudioCaptureUsageDescription"]
LANGS = ["sv", "da", "nb", "fi", "de", "fr", "es"]


def write_tables(plist_path: Path, catalog_path: Path, out_dir: Path) -> None:
    with open(plist_path, "rb") as f:
        info = plistlib.load(f)
    strings = json.loads(catalog_path.read_text(encoding="utf-8")).get("strings", {})
    tables: dict[str, dict[str, str]] = {lang: {} for lang in LANGS}

    for key in KEYS:
        english = info.get(key)
        if not english:
            sys.exit(f"{plist_path}: {key} is missing")
        localizations = strings.get(english, {}).get("localizations", {})
        for lang in LANGS:
            value = localizations.get(lang, {}).get("stringUnit", {}).get("value")
            if not value:
                sys.exit(f"{catalog_path}: no {lang} translation of {key} ({english!r})")
            tables[lang][key] = value

    for lang, table in tables.items():
        lproj = out_dir / f"{lang}.lproj"
        lproj.mkdir(parents=True, exist_ok=True)
        with open(lproj / "InfoPlist.strings", "wb") as f:
            plistlib.dump(table, f, fmt=plistlib.FMT_BINARY, sort_keys=True)


def main() -> None:
    if len(sys.argv) != 4:
        sys.exit(__doc__)
    write_tables(Path(sys.argv[1]), Path(sys.argv[2]), Path(sys.argv[3]))


if __name__ == "__main__":
    main()
