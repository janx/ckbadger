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
