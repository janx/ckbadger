# Footer Broadcast Strip Design

## Goal

- Refresh the site footer into a harder-edged broadcast strip so the left-side attribution still reads `Designed by @busyforking, coded by Claude and Codex. ❤️`, but the whole footer feels like a control-panel status bar instead of a signed plaque.

## Principle Alignment

- CKB Native: no chain semantics, indexing logic, or protocol interpretation changes; this is frontend presentation only.
- Local First: the local build version and shortcut hint remain directly visible in the local UI with no added fetch path.
- Agent Friendly: the footer stays as one component with one explicit attribution string and a stable set of machine-obvious status segments.

## Problem Summary

- The current footer structure is already compact, but its visual grouping is too soft.
- The earlier plaque direction added ceremony, but it pushed the footer toward a card-like signature instead of a system strip.
- The desired direction is more tool-like: segmented, dense, and clearly divided into channels.

## Constraints

- Keep the attribution copy exactly:
  - `Designed by @busyforking, coded by Claude and Codex. ❤️`
- Preserve the existing functional content:
  - build version
  - `Press ? for shortcuts`
  - `Hardforks`
  - `Github`
- Stay within the current slate plus terminal-green palette.
- Avoid decorative floating-card treatments and avoid introducing extra data sources.
- Maintain a usable mobile layout while preserving the sense of segmented channels.

## Approaches Considered

### Approach 1: Balanced strip

- Four continuous slots:
  - credits
  - build
  - shortcut
  - links
- Credits get the widest segment; the rest size to content.

Trade-offs:

- Strong control-panel feel.
- Keeps the attribution readable.
- Best fit for the approved direction.

### Approach 2: Telemetry strip

- Merge credits and build into one left-side channel and keep utility channels on the right.

Trade-offs:

- Very compact and system-like.
- Makes the attribution feel too much like generic metadata.

### Approach 3: Full grid strip

- Equal-width segment grid across all footer items.

Trade-offs:

- Maximum segmented look.
- Too rigid for long build-version strings and weaker on mobile.

## Recommendation

- Use Approach 1: Balanced strip.
- It delivers the strongest broadcast-strip identity without sacrificing readability or forcing awkward truncation.

## Proposed Design

### Structure

- Keep the outer `footer` shell but flatten the inner layout into one continuous strip.
- Use four channel slots in order:
  - `CREDITS`
  - `BUILD`
  - `SHORTCUT`
  - `LINKS`
- Each slot has:
  - a subtle channel label
  - a content row
  - a stronger vertical divider between slots

### Content

- `CREDITS`:
  - `Designed by @busyforking, coded by Claude and Codex. ❤️`
  - `@busyforking` remains the only inline external link in this slot.
- `BUILD`:
  - existing `buildVersion`
- `SHORTCUT`:
  - `Press ? for shortcuts`
- `LINKS`:
  - `Hardforks`
  - `Github`

### Visual Language

- Replace the current plaque/card emphasis with a flatter, longer strip.
- Strengthen borders and separators so each slot reads like a status channel.
- Use uppercase mono labels, tighter tracking, and denser spacing.
- Keep terminal-green reserved for the most important interactive accents rather than broad glow effects.

### Responsive Behavior

- Desktop: a single continuous strip with visible slot boundaries.
- Narrow widths: wrap to two rows while preserving slot borders and labels so the footer still reads as channels, not as loose cards.
- The credits slot should remain first and visually dominant.

### Testing

- Update the footer component test first to assert:
  - the full attribution remains present
  - `buildVersion` remains present
  - `Press ? for shortcuts` remains present
  - `Hardforks` and `Github` remain present
- Add assertions for the channel labels:
  - `Credits`
  - `Build`
  - `Shortcut`
  - `Links`
- Keep tests focused on visible strip structure and core behavior, not fragile style minutiae.
