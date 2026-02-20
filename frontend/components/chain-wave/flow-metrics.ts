export interface FlowMetricTx {
  size: number;
  feeRate?: number | null;
  cycles?: number | null;
}

export interface MetricDomain {
  sizeMin: number;
  sizeMax: number;
  feeRateMin: number;
  feeRateMax: number;
  cyclesMin: number;
  cyclesMax: number;
}

export interface ScatterPoint {
  x: number;
  y: number;
  radius: number;
  sizeScore: number;
  feeScore: number;
  cyclesScore: number;
  missingFeeRate: boolean;
  missingCycles: boolean;
}

const DEFAULT_DOMAIN: MetricDomain = {
  sizeMin: 200,
  sizeMax: 10_000,
  feeRateMin: 1,
  feeRateMax: 5_000,
  cyclesMin: 10_000,
  cyclesMax: 5_000_000,
};

const EPSILON = 0.000001;

function clamp01(value: number): number {
  if (Number.isNaN(value) || !Number.isFinite(value)) return 0;
  return Math.min(1, Math.max(0, value));
}

function normalizeLinear(value: number, min: number, max: number): number {
  if (max <= min) return 0.5;
  return clamp01((value - min) / (max - min));
}

function normalizeLog(value: number, min: number, max: number): number {
  if (value <= 0 || min <= 0 || max <= 0) return normalizeLinear(value, min, max);
  const logMin = Math.log(min);
  const logMax = Math.log(max);
  const logValue = Math.log(value);
  if (logMax <= logMin) return 0.5;
  return clamp01((logValue - logMin) / (logMax - logMin));
}

function toFinitePositive(values: Array<number | null | undefined>): number[] {
  return values
    .filter(
      (value): value is number => value !== null && value !== undefined && Number.isFinite(value)
    )
    .map((value) => Math.max(value, EPSILON));
}

function minMax(values: number[], fallbackMin: number, fallbackMax: number): [number, number] {
  if (values.length === 0) return [fallbackMin, fallbackMax];

  const minValue = Math.min(...values);
  const maxValue = Math.max(...values);

  if (minValue === maxValue) {
    if (minValue <= 1) return [EPSILON, minValue + 1];
    return [minValue * 0.8, minValue * 1.2];
  }

  return [minValue, maxValue];
}

export function buildMetricDomain(items: FlowMetricTx[]): MetricDomain {
  if (items.length === 0) return DEFAULT_DOMAIN;

  const sizes = toFinitePositive(items.map((item) => item.size));
  const feeRates = toFinitePositive(items.map((item) => item.feeRate));
  const cycles = toFinitePositive(items.map((item) => item.cycles));

  const [sizeMin, sizeMax] = minMax(sizes, DEFAULT_DOMAIN.sizeMin, DEFAULT_DOMAIN.sizeMax);
  const [feeRateMin, feeRateMax] = minMax(
    feeRates,
    DEFAULT_DOMAIN.feeRateMin,
    DEFAULT_DOMAIN.feeRateMax
  );
  const [cyclesMin, cyclesMax] = minMax(cycles, DEFAULT_DOMAIN.cyclesMin, DEFAULT_DOMAIN.cyclesMax);

  return {
    sizeMin,
    sizeMax,
    feeRateMin,
    feeRateMax,
    cyclesMin,
    cyclesMax,
  };
}

export function mapTxToScatterPoint(tx: FlowMetricTx, domain: MetricDomain): ScatterPoint {
  const safeSize = Math.max(tx.size, EPSILON);
  const hasFeeRate = tx.feeRate !== null && tx.feeRate !== undefined && tx.feeRate > 0;
  const hasCycles = tx.cycles !== null && tx.cycles !== undefined && tx.cycles > 0;

  const sizeScore = normalizeLog(safeSize, domain.sizeMin, domain.sizeMax);
  const feeScore = hasFeeRate
    ? normalizeLog(Math.max(tx.feeRate ?? EPSILON, EPSILON), domain.feeRateMin, domain.feeRateMax)
    : 0;
  const cyclesScore = hasCycles
    ? normalizeLog(Math.max(tx.cycles ?? EPSILON, EPSILON), domain.cyclesMin, domain.cyclesMax)
    : 0;

  return {
    x: feeScore,
    y: 1 - cyclesScore,
    radius: 3 + sizeScore * 10,
    sizeScore,
    feeScore,
    cyclesScore,
    missingFeeRate: !hasFeeRate,
    missingCycles: !hasCycles,
  };
}
