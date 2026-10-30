/**
 * Node-link layout for small graphs (the Fiber network: tens of parties). Pure and deterministic,
 * so the same API response draws the same picture on every load (FF-F2).
 */

export interface LayoutPoint {
  x: number;
  y: number;
}

type Edge = readonly [string, string];

function adjacency(nodes: readonly string[], edges: readonly Edge[]): Map<string, string[]> {
  const adj = new Map(nodes.map((node) => [node, [] as string[]]));
  if (adj.size !== nodes.length) throw new Error('graph layout: duplicate node');
  for (const [a, b] of edges) {
    for (const end of [a, b])
      if (!adj.has(end)) throw new Error(`graph layout: edge ${a} – ${b} has unknown node ${end}`);
    if (a === b) continue;
    adj.get(a)!.push(b);
    adj.get(b)!.push(a);
  }
  return adj;
}

/** Connected components, largest first (ties keep input order); members keep input order. */
export function connectedComponents(nodes: readonly string[], edges: readonly Edge[]): string[][] {
  const adj = adjacency(nodes, edges);
  const order = new Map(nodes.map((node, index) => [node, index]));
  const seen = new Set<string>();
  const components: string[][] = [];
  for (const start of nodes) {
    if (seen.has(start)) continue;
    const members: string[] = [];
    const queue = [start];
    seen.add(start);
    while (queue.length) {
      const node = queue.shift()!;
      members.push(node);
      for (const next of adj.get(node)!)
        if (!seen.has(next)) {
          seen.add(next);
          queue.push(next);
        }
    }
    components.push(members.sort((a, b) => order.get(a)! - order.get(b)!));
  }
  return components
    .map((members, index) => ({ members, index }))
    .sort((a, b) => b.members.length - a.members.length || a.index - b.index)
    .map(({ members }) => members);
}

/**
 * Kamada–Kawai spring layout (1989) of one connected graph: graph distance becomes layout
 * distance. Unweighted shortest paths; circular start in input order; Newton–Raphson on the node
 * with the largest energy gradient until every gradient is below `epsilon`.
 * Coordinates are in a unit-scale frame; the caller fits them to its canvas.
 */
export function kamadaKawaiLayout(
  nodes: readonly string[],
  edges: readonly Edge[],
  { epsilon = 1e-4, maxSteps = 4000 }: { epsilon?: number; maxSteps?: number } = {}
): Map<string, LayoutPoint> {
  const adj = adjacency(nodes, edges);
  const n = nodes.length;
  if (n === 0) return new Map();
  if (n === 1) return new Map([[nodes[0], { x: 0.5, y: 0.5 }]]);
  const index = new Map(nodes.map((node, i) => [node, i]));
  const d: number[][] = nodes.map((source) => {
    const row = new Array<number>(n).fill(Infinity);
    row[index.get(source)!] = 0;
    const queue = [source];
    while (queue.length) {
      const node = queue.shift()!;
      for (const next of adj.get(node)!) {
        const j = index.get(next)!;
        if (row[j] === Infinity) {
          row[j] = row[index.get(node)!] + 1;
          queue.push(next);
        }
      }
    }
    if (row.includes(Infinity))
      throw new Error(`graph layout: ${source}'s graph is not connected; lay out one component`);
    return row;
  });
  const maxD = Math.max(...d.flat());
  const L = 1 / maxD;
  const l = d.map((row) => row.map((v) => L * v));
  const k = d.map((row) => row.map((v) => (v === 0 ? 0 : 1 / (v * v))));
  const x = nodes.map((_, i) => 0.5 + 0.5 * Math.cos((2 * Math.PI * i) / n));
  const y = nodes.map((_, i) => 0.5 + 0.5 * Math.sin((2 * Math.PI * i) / n));

  const derivatives = (m: number) => {
    let ex = 0;
    let ey = 0;
    let exx = 0;
    let eyy = 0;
    let exy = 0;
    for (let i = 0; i < n; i++) {
      if (i === m) continue;
      const dx = x[m] - x[i];
      const dy = y[m] - y[i];
      const dist = Math.hypot(dx, dy) || 1e-9;
      const dist3 = dist * dist * dist;
      ex += k[m][i] * (dx - (l[m][i] * dx) / dist);
      ey += k[m][i] * (dy - (l[m][i] * dy) / dist);
      exx += k[m][i] * (1 - (l[m][i] * dy * dy) / dist3);
      eyy += k[m][i] * (1 - (l[m][i] * dx * dx) / dist3);
      exy += k[m][i] * ((l[m][i] * dx * dy) / dist3);
    }
    return { ex, ey, exx, eyy, exy, delta: Math.hypot(ex, ey) };
  };

  for (let step = 0; step < maxSteps; step++) {
    let m = -1;
    let worst = epsilon;
    for (let i = 0; i < n; i++) {
      const delta = derivatives(i).delta;
      if (delta > worst) {
        worst = delta;
        m = i;
      }
    }
    if (m < 0) break;
    for (let inner = 0; inner < 50; inner++) {
      const { ex, ey, exx, eyy, exy, delta } = derivatives(m);
      if (delta <= epsilon) break;
      const det = exx * eyy - exy * exy;
      if (Math.abs(det) < 1e-12) break;
      x[m] += (-ex * eyy + ey * exy) / det;
      y[m] += (-ey * exx + ex * exy) / det;
    }
  }
  return new Map(nodes.map((node, i) => [node, { x: x[i], y: y[i] }]));
}
