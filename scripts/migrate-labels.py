#!/usr/bin/env python3
"""Migrate token-labels JSON to docs/metadata/ TOML files.

Reads from:
  - docs/token-labels/information/udt/{mainnet,testnet}/*/index.json
  - docs/token-labels/information/script/*/index.json
  - docs/script-name-overrides.json

Writes to:
  - docs/metadata/tokens/{slug}.toml
  - docs/metadata/scripts/{slug}.toml
  - docs/metadata/nft-tiers.toml

Uses only Python 3 stdlib.
"""

import json
import os
import re
import sys
from collections import defaultdict
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
TOKEN_LABELS = ROOT / "docs" / "token-labels" / "information"
OVERRIDES_PATH = ROOT / "docs" / "script-name-overrides.json"
OUTPUT = ROOT / "docs" / "metadata"


# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------

def make_slug(text: str) -> str:
    """Generate a URL-safe slug from text."""
    s = text.lower()
    s = re.sub(r"[^a-z0-9]", "-", s)
    s = re.sub(r"-+", "-", s)
    s = s.strip("-")
    return s


def is_zero_hash(h: str) -> bool:
    """Return True if hash is empty or all-zero."""
    if not h:
        return True
    stripped = h.lower().removeprefix("0x")
    return all(c == "0" for c in stripped)


def escape_toml_string(s: str) -> str:
    """Escape a string for TOML double-quoted value."""
    result = []
    for c in s:
        if c == '\\': result.append('\\\\')
        elif c == '"': result.append('\\"')
        elif c == '\n': result.append('\\n')
        elif c == '\r': result.append('\\r')
        elif c == '\t': result.append('\\t')
        elif ord(c) < 0x20 or ord(c) == 0x7F:
            result.append(f'\\u{ord(c):04X}')
        else:
            result.append(c)
    return ''.join(result)


def toml_str(val: str) -> str:
    """Format a string as a TOML quoted value."""
    return '"' + escape_toml_string(val) + '"'


# ---------------------------------------------------------------------------
# Token migration
# ---------------------------------------------------------------------------

def load_tokens():
    """Load all published UDT tokens, grouped by (name, symbol).

    Returns dict: (name, symbol) -> {network: [token_data, ...]}
    """
    tokens = defaultdict(lambda: defaultdict(list))

    for network in ("mainnet", "testnet"):
        udt_dir = TOKEN_LABELS / "udt" / network
        if not udt_dir.exists():
            continue
        for entry in sorted(udt_dir.iterdir()):
            index_file = entry / "index.json"
            if not index_file.is_file():
                continue
            data = json.loads(index_file.read_text())
            if not data.get("published"):
                continue

            name = data.get("name") or data.get("symbol") or ""
            symbol = data.get("symbol") or ""
            if not name or not symbol:
                continue

            key = (name, symbol)
            tokens[key][network].append(data)

    return tokens


def write_token_toml(path: Path, name: str, symbol: str, entries_by_net: dict):
    """Write a single token TOML file.

    If multiple entries exist for a network (same name+symbol but different
    type scripts), we pick the first one (they should be identical in the
    merged grouping; if not, the first is fine).
    """
    lines = []
    lines.append(f"name = {toml_str(name)}")
    lines.append(f"symbol = {toml_str(symbol)}")

    # Use the first entry that has the field for metadata
    all_entries = []
    for net in ("mainnet", "testnet"):
        all_entries.extend(entries_by_net.get(net, []))

    if not all_entries:
        return

    first = all_entries[0]

    decimal_val = first.get("decimal")
    if decimal_val is not None:
        lines.append(f"decimals = {int(decimal_val)}")

    udt_type = first.get("udtType")
    if udt_type:
        lines.append(f"standard = {toml_str(udt_type)}")

    icon = first.get("icon")
    if icon:
        lines.append(f"icon = {toml_str(icon)}")

    desc = first.get("description")
    if desc:
        lines.append(f"description = {toml_str(desc)}")

    for net in ("mainnet", "testnet"):
        net_entries = entries_by_net.get(net, [])
        if not net_entries:
            continue
        entry = net_entries[0]
        type_info = entry.get("type", {})
        code_hash = type_info.get("codeHash", "")
        hash_type = type_info.get("hashType", "")
        args = type_info.get("args", "")

        lines.append("")
        lines.append(f"[{net}]")
        lines.append(f"code_hash = {toml_str(code_hash)}")
        lines.append(f"hash_type = {toml_str(hash_type)}")
        lines.append(f"args = {toml_str(args)}")

    path.write_text("\n".join(lines) + "\n")


def migrate_tokens():
    """Migrate all UDT tokens to TOML files."""
    tokens = load_tokens()
    out_dir = OUTPUT / "tokens"
    out_dir.mkdir(parents=True, exist_ok=True)

    # Build slug -> list of keys to detect collisions
    slug_to_keys = defaultdict(list)
    for key in tokens:
        name, symbol = key
        slug = make_slug(symbol)
        slug_to_keys[slug].append(key)

    collisions = []
    written = 0
    mainnet_count = 0
    testnet_count = 0

    for slug, keys in sorted(slug_to_keys.items()):
        if not slug:
            # Empty slug (non-alphanumeric symbols only) -- use type_hash
            for key in keys:
                entries_by_net = tokens[key]
                all_entries = []
                for net in ("mainnet", "testnet"):
                    all_entries.extend(entries_by_net.get(net, []))
                if not all_entries:
                    continue
                type_hash = all_entries[0].get("typeHash", "")
                suffix = type_hash[2:10] if type_hash.startswith("0x") else type_hash[:8]
                final_slug = f"token-{suffix}" if suffix else f"token-unknown-{written}"
                path = out_dir / f"{final_slug}.toml"
                name, symbol = key
                write_token_toml(path, name, symbol, entries_by_net)
                written += 1
                if "mainnet" in entries_by_net:
                    mainnet_count += 1
                if "testnet" in entries_by_net:
                    testnet_count += 1
            continue

        if len(keys) == 1:
            key = keys[0]
            entries_by_net = tokens[key]
            path = out_dir / f"{slug}.toml"
            name, symbol = key
            write_token_toml(path, name, symbol, entries_by_net)
            written += 1
            if "mainnet" in entries_by_net:
                mainnet_count += 1
            if "testnet" in entries_by_net:
                testnet_count += 1
        else:
            collisions.append((slug, keys))
            for key in keys:
                entries_by_net = tokens[key]
                all_entries = []
                for net in ("mainnet", "testnet"):
                    all_entries.extend(entries_by_net.get(net, []))
                if not all_entries:
                    continue
                type_hash = all_entries[0].get("typeHash", "")
                suffix = type_hash[2:10] if type_hash.startswith("0x") else type_hash[:8]
                final_slug = f"{slug}-{suffix}" if suffix else slug
                path = out_dir / f"{final_slug}.toml"
                name, symbol = key
                write_token_toml(path, name, symbol, entries_by_net)
                written += 1
                if "mainnet" in entries_by_net:
                    mainnet_count += 1
                if "testnet" in entries_by_net:
                    testnet_count += 1

    return written, mainnet_count, testnet_count, collisions


# ---------------------------------------------------------------------------
# Script migration
# ---------------------------------------------------------------------------

def load_overrides():
    """Load script-name-overrides.json."""
    data = json.loads(OVERRIDES_PATH.read_text())
    return {
        "name_map": data.get("overrides", {}),
        "known_scripts": data.get("known_scripts", []),
        "deprecated": set(h.lower() for h in data.get("deprecated", [])),
        "nft_tiers": data.get("nft_storage_tier_overrides", {}),
    }


def load_scripts(overrides):
    """Load all scripts from token-labels + known_scripts.

    Returns dict: final_name -> script_data (with merged deployments).
    """
    name_map = overrides["name_map"]
    deprecated_hashes = overrides["deprecated"]
    known_scripts = overrides["known_scripts"]

    scripts = {}  # name -> {name, description, website, category, deployments: {net: [...]}}

    # Collect all code_hashes from regular scripts to check known_scripts overlap
    regular_code_hashes = set()

    script_dir = TOKEN_LABELS / "script"
    for entry in sorted(script_dir.iterdir()):
        index_file = entry / "index.json"
        if not index_file.is_file():
            continue
        data = json.loads(index_file.read_text())
        raw_name = data.get("name", "")

        # Apply name override
        final_name = name_map.get(raw_name, raw_name)

        description = data.get("description", "")
        website = data.get("website", "")
        category = data.get("decoderType", "")

        deployments = {"mainnet": [], "testnet": []}
        for net in ("mainnet", "testnet"):
            for dep in data.get("deployments", {}).get(net, []):
                code_hash = dep.get("codeHash", "")
                regular_code_hashes.add(code_hash.lower())
                data_hash = dep.get("dataHash", "")
                hash_type = dep.get("hashType", "")
                is_deprecated = dep.get("deprecated", False)
                tag = dep.get("tag", "")

                # Apply deprecated overrides
                if code_hash.lower() in deprecated_hashes:
                    is_deprecated = True

                entry_data = {
                    "code_hash": code_hash,
                    "hash_type": hash_type,
                }
                if not is_zero_hash(data_hash):
                    entry_data["data_hash"] = data_hash
                if is_deprecated:
                    entry_data["deprecated"] = True
                if tag:
                    entry_data["tag"] = tag

                deployments[net].append(entry_data)

        if final_name in scripts:
            # Merge deployments
            for net in ("mainnet", "testnet"):
                scripts[final_name]["deployments"][net].extend(deployments[net])
            # Update metadata if empty
            if not scripts[final_name]["description"] and description:
                scripts[final_name]["description"] = description
            if not scripts[final_name]["website"] and website:
                scripts[final_name]["website"] = website
            if not scripts[final_name]["category"] and category:
                scripts[final_name]["category"] = category
        else:
            scripts[final_name] = {
                "name": final_name,
                "description": description,
                "website": website,
                "category": category,
                "deployments": deployments,
            }

    # Process known_scripts
    for ks in known_scripts:
        raw_name = ks.get("name", "")
        final_name = name_map.get(raw_name, raw_name)
        description = ks.get("description", "")
        website = ks.get("website", "")
        category = ks.get("decoderType", "")

        deployments = {"mainnet": [], "testnet": []}
        skip_all = True  # Will be set to False if at least one non-overlapping deployment
        for net in ("mainnet", "testnet"):
            for dep in ks.get("deployments", {}).get(net, []):
                code_hash = dep.get("codeHash", "")
                if code_hash.lower() in regular_code_hashes:
                    continue  # Skip overlapping deployment
                skip_all = False
                data_hash = dep.get("dataHash", "")
                hash_type = dep.get("hashType", "")
                is_deprecated = dep.get("deprecated", False)
                tag = dep.get("tag", "")

                if code_hash.lower() in deprecated_hashes:
                    is_deprecated = True

                entry_data = {
                    "code_hash": code_hash,
                    "hash_type": hash_type,
                }
                if not is_zero_hash(data_hash):
                    entry_data["data_hash"] = data_hash
                if is_deprecated:
                    entry_data["deprecated"] = True
                if tag:
                    entry_data["tag"] = tag

                deployments[net].append(entry_data)

        if skip_all:
            continue

        if final_name in scripts:
            # Merge deployments into existing script
            for net in ("mainnet", "testnet"):
                scripts[final_name]["deployments"][net].extend(deployments[net])
            if not scripts[final_name]["description"] and description:
                scripts[final_name]["description"] = description
            if not scripts[final_name]["website"] and website:
                scripts[final_name]["website"] = website
            if not scripts[final_name]["category"] and category:
                scripts[final_name]["category"] = category
        else:
            scripts[final_name] = {
                "name": final_name,
                "description": description,
                "website": website,
                "category": category,
                "deployments": deployments,
            }

    return scripts


def write_script_toml(path: Path, script: dict):
    """Write a single script TOML file."""
    lines = []
    lines.append(f"name = {toml_str(script['name'])}")

    desc = script.get("description", "")
    if desc:
        lines.append(f"description = {toml_str(desc)}")

    website = script.get("website", "")
    if website:
        lines.append(f"website = {toml_str(website)}")

    category = script.get("category", "")
    if category:
        lines.append(f"category = {toml_str(category)}")

    for net in ("mainnet", "testnet"):
        deps = script["deployments"].get(net, [])
        for dep in deps:
            lines.append("")
            lines.append(f"[[{net}]]")
            lines.append(f"code_hash = {toml_str(dep['code_hash'])}")
            if "data_hash" in dep:
                lines.append(f"data_hash = {toml_str(dep['data_hash'])}")
            lines.append(f"hash_type = {toml_str(dep['hash_type'])}")
            if dep.get("deprecated"):
                lines.append("deprecated = true")
            if dep.get("tag"):
                lines.append(f"tag = {toml_str(dep['tag'])}")

    path.write_text("\n".join(lines) + "\n")


def migrate_scripts():
    """Migrate all scripts to TOML files."""
    overrides = load_overrides()
    scripts = load_scripts(overrides)
    out_dir = OUTPUT / "scripts"
    out_dir.mkdir(parents=True, exist_ok=True)

    written = 0
    from_known = 0

    for name, script in sorted(scripts.items()):
        slug = make_slug(name)
        if not slug:
            slug = f"script-{written}"
        path = out_dir / f"{slug}.toml"
        write_script_toml(path, script)
        written += 1

    # Write nft-tiers.toml
    nft_tiers = overrides["nft_tiers"]
    if nft_tiers:
        lines = ["[overrides]"]
        for key in sorted(nft_tiers.keys()):
            lines.append(f"{toml_str(key)} = {toml_str(nft_tiers[key])}")
        nft_path = OUTPUT / "nft-tiers.toml"
        nft_path.write_text("\n".join(lines) + "\n")

    return written, len(overrides["known_scripts"])


# ---------------------------------------------------------------------------
# Main
# ---------------------------------------------------------------------------

def main():
    print("Migrating token-labels JSON to docs/metadata/ TOML...\n")

    # Tokens
    token_count, mainnet_tokens, testnet_tokens, collisions = migrate_tokens()
    print(f"Tokens: {token_count} files written")
    print(f"  Mainnet tokens: {mainnet_tokens}")
    print(f"  Testnet tokens: {testnet_tokens}")
    if collisions:
        print(f"  Slug collisions resolved: {len(collisions)}")
        for slug, keys in collisions[:10]:
            names = ", ".join(f"{n}/{s}" for n, s in keys)
            print(f"    {slug}: {names}")
        if len(collisions) > 10:
            print(f"    ... and {len(collisions) - 10} more")

    # Scripts
    script_count, known_count = migrate_scripts()
    print(f"\nScripts: {script_count} files written (including from {known_count} known_scripts entries)")

    # NFT tiers
    nft_path = OUTPUT / "nft-tiers.toml"
    if nft_path.exists():
        print(f"NFT tiers: {nft_path}")

    print("\nDone.")


if __name__ == "__main__":
    main()
