export type ScriptRefHashType = 'type' | 'data' | 'data1' | 'data2';

export function normalizeScriptRefHashType(
  value: string | null | undefined
): ScriptRefHashType | null {
  if (value === 'type' || value === 'data' || value === 'data1' || value === 'data2') {
    return value;
  }
  return null;
}

export function getScriptRefQueryHashType(
  value: string | null | undefined,
  fallback: ScriptRefHashType = 'type'
): ScriptRefHashType {
  return normalizeScriptRefHashType(value) ?? fallback;
}

export function getScriptRefBadgeLabel(value: string | null | undefined): string {
  const normalized = normalizeScriptRefHashType(value);
  if (normalized === 'type') {
    return 'type';
  }
  return `bytecode(${normalized ?? 'data'})`;
}

export function getScriptRefVerboseLabel(value: string | null | undefined): string {
  const normalized = normalizeScriptRefHashType(value);
  if (normalized === 'type') {
    return 'type ref';
  }
  return `bytecode hash ref (${normalized ?? 'data'})`;
}
