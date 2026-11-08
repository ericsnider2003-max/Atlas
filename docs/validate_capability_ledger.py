#!/usr/bin/env python3
"""Validate the replica capability ledger against generated CAPABILITIES.md.

This is intentionally read-only.  The classification CSV is planning/evidence
metadata, not runtime truth, so this tool checks identity and completeness but
never upgrades a capability's source, installed, or owner proof.

Usage:
    python docs/validate_capability_ledger.py \
        --ledger ../replica/ATLAS_CAPABILITY_CLASSIFICATION.csv \
        --catalog docs/CAPABILITIES.md
"""

from __future__ import annotations

import argparse
import csv
import re
import sys
from collections import Counter
from pathlib import Path
from typing import NoReturn


REQUIRED = {
    "id",
    "catalog_text",
    "classification",
    "dependency",
    "source_proof",
    "installed_proof",
    "owner_proof",
    "status",
    "rollback",
}
ROW = re.compile(
    r"^\| (?P<text>.+?) \| "
    r"(?:working|switched off|waiting on .+|built, never run for real|not built yet) \|"
)


def fail(message: str) -> "NoReturn":
    raise SystemExit(f"ledger validation failed: {message}")


def read_catalog(path: Path) -> list[str]:
    texts: list[str] = []
    for line in path.read_text(encoding="utf-8").splitlines():
        match = ROW.match(line)
        if match:
            texts.append(match.group("text"))
    return texts


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--ledger", type=Path, required=True)
    parser.add_argument("--catalog", type=Path, required=True)
    args = parser.parse_args()

    with args.ledger.open(newline="", encoding="utf-8") as handle:
        reader = csv.DictReader(handle)
        fields = set(reader.fieldnames or ())
        missing = REQUIRED - fields
        if missing:
            fail(f"ledger is missing required columns: {', '.join(sorted(missing))}")
        rows = list(reader)

    if not rows:
        fail("ledger has no capability rows")

    ids = [row["id"] for row in rows]
    bad_ids = [value for value in ids if not re.fullmatch(r"C\d{3}", value)]
    if bad_ids:
        fail(f"invalid stable IDs: {bad_ids[:5]}")
    if len(set(ids)) != len(ids):
        fail("stable IDs are not unique")

    for number, row in enumerate(rows, start=2):
        for field in REQUIRED - {"id"}:
            if not row.get(field, "").strip():
                fail(f"row {number} has an empty {field} field")

    catalog = read_catalog(args.catalog)
    ledger_text = [row["catalog_text"] for row in rows]
    if len(catalog) != len(rows):
        fail(f"catalog has {len(catalog)} rows but ledger has {len(rows)}")
    missing = [text for text in ledger_text if text not in catalog]
    extra = [text for text in catalog if text not in ledger_text]
    if missing:
        fail(f"ledger text absent from catalog: {missing[:3]}")
    if extra:
        fail(f"catalog text absent from ledger: {extra[:3]}")

    counts = Counter(row["classification"] for row in rows)
    summary = ", ".join(f"{key}={counts[key]}" for key in sorted(counts))
    print(f"validated {len(rows)} capability rows; {len(set(ids))} unique IDs; {summary}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
