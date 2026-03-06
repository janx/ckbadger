# Frontend Bundle Baseline

## 2026-03-06 Before Bundle Optimization

Command:

```bash
cd frontend && pnpm build
```

Observed output before bundle optimization work:

```text
dist/index.html                                 0.53 kB │ gzip:   0.33 kB
dist/assets/index-BbFERrQC.css                 71.27 kB │ gzip:  12.86 kB
dist/assets/react-force-graph-2d-BOoY7nwq.js  188.39 kB │ gzip:  61.99 kB
dist/assets/index-Diwe2U5p.js                 813.28 kB │ gzip: 215.99 kB
```

Interpretation:

- The main entry chunk is still roughly `813 kB` minified.
- `react-force-graph-2d` is already split into its own asset, but the main entry remains large enough to justify more explicit lazy boundaries and route-local deferral.

## 2026-03-06 After Bundle Optimization

Command:

```bash
cd frontend && pnpm build
```

Observed output after graph wrapper splitting, detail-page graph deferral, and router-level route splitting:

```text
dist/index.html                                           0.53 kB │ gzip:   0.33 kB
dist/assets/index-YgjK8c9j.css                           71.30 kB │ gzip:  12.86 kB
dist/assets/react-force-graph-2d-adhddBJ2.js            188.39 kB │ gzip:  61.99 kB
dist/assets/index-C6UU-CGz.js                           398.15 kB │ gzip: 125.17 kB
```

Interpretation:

- The main entry chunk dropped from `813.28 kB` to `398.15 kB`.
- `react-force-graph-2d` remains split into its own asset, and the main route shell now stays below Vite's `500 kB` warning threshold.
- The largest remaining assets are route-local page chunks, so any further optimization should target specific slow routes rather than more shell-level splitting.
