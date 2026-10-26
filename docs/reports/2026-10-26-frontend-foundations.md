## Goal

- Implement Phase A of the approved site redesign: shared presentation components, exact visual encodings, local fonts and corrected legacy/error routing.

## Principle Alignment

- Domain facts precede developer evidence. `DetailLayout` enforces the 72 px tier boundary and collapses raw data.
- Exact amounts remain decimal strings/BigInts. Numbers are converted to floating point only for drawing geometry; missing chart values, broken partitions, malformed bytes and dangling script references fail with entity context.
- Activity rendering is promoted from the homepage into one reusable component. All shared link components preserve the active network.
- No store writes or schema changes. No services restarted, no running data purged or rebuilt. Fonts are bundled locally under their SIL Open Font License.

## Result

- Added `DetailLayout`, `DetailSection`, `ListTable`, `ActivityRow`, `TxStructure`, `CapacityPartition`, `CellBar`, `CellTile`, `ClassMark`, `Chart`, `HexMap`, `DnaStrip` and `AlleleGel`, with meaningful interaction and invalid-state tests.
- Added canonical class/cell/copper palettes and typography sizes; hashes use neutral ink. CRT effects and intentional logo behavior remain intact.
- Restored `/tokens`, `/objects`, `/dao/charts` aliases with network, query and fragment preservation. Unexpected route failures render a distinct crash component; actual 404 responses retain not-found handling.
- Bundled JetBrains Mono and Noto Serif Display as WOFF2. Removed runtime Google Fonts requests.
- Validation: baseline 163 files / 1,352 tests passed; two Vite listener tests initially hit sandbox EPERM. Final frontend type-check, lint and full tests passed outside the sandbox: 168 files / 1,381 tests. Neutral-hash and redirect/crash regression tests failed before implementation. Focused interaction tests cover all new components.
- Re-sync required: no.
- What to do next: consume these components in D1/D2 pages, then migrate the later page families as their API contracts land. The existing chart/card adapters remain for pages scheduled in later rounds. The final vessels artwork/error-page composition belongs to D8; A3 supplies the distinct crash state now. Full-page 1440/390 acceptance accompanies the D page commits.
