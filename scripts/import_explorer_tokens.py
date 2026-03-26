#!/usr/bin/env python3
"""
One-time script to import published token metadata from the official CKB explorer API
into docs/metadata/tokens/ as TOML files.

Usage: python3 scripts/import_explorer_tokens.py [--dry-run]
"""

import json
import os
import re
import sys
import time
import urllib.request

EXPLORER_API = "https://mainnet-api.explorer.nervos.org"
METADATA_DIR = os.path.join(os.path.dirname(__file__), "..", "docs", "metadata", "tokens")
PAGE_SIZE = 100
ACCEPT_HEADER = "application/vnd.api+json"

# Map explorer udt_type to ckbadger standard
UDT_TYPE_MAP = {
    "sudt": "sudt",
    "xudt": "xudt",
    "xudt_compatible": "xudt",
    "omiga_inscription": "xudt",
}


def fetch_page(page: int, udt_type: str = "xudt") -> dict:
    url = f"{EXPLORER_API}/api/v1/udts?page={page}&page_size={PAGE_SIZE}&type_hash=&udt_type={udt_type}"
    req = urllib.request.Request(url, headers={"Accept": ACCEPT_HEADER})
    with urllib.request.urlopen(req, timeout=30) as resp:
        return json.loads(resp.read().decode())


def load_existing_tokens() -> set:
    """Load (code_hash, hash_type, args) tuples from existing TOML files."""
    existing = set()
    if not os.path.isdir(METADATA_DIR):
        return existing
    for fname in os.listdir(METADATA_DIR):
        if not fname.endswith(".toml"):
            continue
        path = os.path.join(METADATA_DIR, fname)
        code_hash = hash_type = args = None
        in_mainnet = False
        with open(path) as f:
            for line in f:
                line = line.strip()
                if line == "[mainnet]":
                    in_mainnet = True
                elif line.startswith("[") and line != "[mainnet]":
                    in_mainnet = False
                elif in_mainnet:
                    if line.startswith("code_hash"):
                        code_hash = line.split("=", 1)[1].strip().strip('"')
                    elif line.startswith("hash_type"):
                        hash_type = line.split("=", 1)[1].strip().strip('"')
                    elif line.startswith("args"):
                        args = line.split("=", 1)[1].strip().strip('"')
        if code_hash and hash_type and args:
            existing.add((code_hash, hash_type, args))
    return existing


def make_filename(symbol: str, args: str) -> str:
    slug = re.sub(r"[^a-z0-9]+", "-", symbol.lower()).strip("-")
    if not slug:
        slug = "unknown"
    args_prefix = args[2:10] if args.startswith("0x") else args[:8]
    return f"{slug}-{args_prefix}.toml"


def generate_toml(token: dict) -> str:
    attrs = token["attributes"]
    name = attrs.get("full_name") or ""
    symbol = attrs.get("symbol") or ""
    decimal = attrs.get("decimal") or "0"
    udt_type = attrs.get("udt_type") or "xudt"
    standard = UDT_TYPE_MAP.get(udt_type, "xudt")

    ts = attrs.get("type_script") or {}
    code_hash = ts.get("code_hash", "")
    hash_type = ts.get("hash_type", "")
    args = ts.get("args", "")

    lines = [
        f'name = "{name}"',
        f'symbol = "{symbol}"',
        f"decimals = {decimal}",
        f'standard = "{standard}"',
        "",
        "[mainnet]",
        f'code_hash = "{code_hash}"',
        f'hash_type = "{hash_type}"',
        f'args = "{args}"',
        "",
    ]
    return "\n".join(lines)


def main():
    dry_run = "--dry-run" in sys.argv
    existing = load_existing_tokens()
    print(f"Found {len(existing)} existing token TOML files")

    imported = 0
    skipped_existing = 0
    skipped_empty = 0

    for udt_type in ["xudt", "sudt"]:
        page = 1
        while True:
            print(f"Fetching {udt_type} page {page}...")
            try:
                data = fetch_page(page, udt_type)
            except Exception as e:
                print(f"  Error fetching page {page}: {e}")
                break

            tokens = data.get("data", [])
            if not tokens:
                break

            for token in tokens:
                attrs = token["attributes"]

                if not attrs.get("published"):
                    continue

                symbol = (attrs.get("symbol") or "").strip()
                name = (attrs.get("full_name") or "").strip()
                if not symbol or not name:
                    skipped_empty += 1
                    continue

                ts = attrs.get("type_script") or {}
                code_hash = ts.get("code_hash", "")
                hash_type = ts.get("hash_type", "")
                args = ts.get("args", "")

                if not code_hash or not args:
                    skipped_empty += 1
                    continue

                if (code_hash, hash_type, args) in existing:
                    skipped_existing += 1
                    continue

                filename = make_filename(symbol, args)
                filepath = os.path.join(METADATA_DIR, filename)

                # Avoid overwriting if filename collision
                if os.path.exists(filepath):
                    skipped_existing += 1
                    continue

                toml_content = generate_toml(token)
                if dry_run:
                    print(f"  [dry-run] Would create: {filename}")
                else:
                    with open(filepath, "w") as f:
                        f.write(toml_content)
                    print(f"  Created: {filename}")
                imported += 1
                existing.add((code_hash, hash_type, args))

            meta = data.get("meta", {})
            total_pages = meta.get("total_pages", 1)
            if page >= total_pages:
                break
            page += 1
            time.sleep(0.5)  # Rate limit

    print(f"\nDone: {imported} imported, {skipped_existing} skipped (existing), {skipped_empty} skipped (empty)")


if __name__ == "__main__":
    main()
