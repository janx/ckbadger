import { describe, expect, it } from 'vitest';
import { analyzeWitness, buildScriptGroupLens, inferWitnessInsights } from '@/lib/witness-analysis';

describe('witness-analysis', () => {
  it('decodes WitnessArgs deterministic structure', () => {
    const analysis = analyzeWitness(
      '0x1b00000010000000160000001600000006000000112205000000aa',
      0,
      1
    );

    expect(analysis.role).toBe('input');
    expect(analysis.byteLength).toBe(27);
    expect(analysis.deterministic?.kind).toBe('WitnessArgs');
    expect(analysis.deterministic?.segments.some((segment) => segment.label === 'lock')).toBe(true);
  });

  it('decodes DAS witness and emits DAS heuristic', () => {
    const analysis = analyzeWitness('0x64617301020304', 1, 1);

    expect(analysis.role).toBe('extra');
    expect(analysis.deterministic?.kind).toBe('DASWitness');
    expect(analysis.heuristicGuesses.some((guess) => guess.kind === 'das_witness')).toBe(true);
  });

  it('builds script-group lens keyed by first input witness index', () => {
    const lens = buildScriptGroupLens({
      inputs: [
        {
          lock: { codeHash: '0xaaa', hashType: 'type', args: '0x01' },
        },
        {
          lock: { codeHash: '0xaaa', hashType: 'type', args: '0x01' },
          type: { codeHash: '0xbbb', hashType: 'data', args: '0x02' },
        },
        {
          lock: { codeHash: '0xccc', hashType: 'type', args: '0x03' },
        },
      ],
    });

    expect(lens).toHaveLength(3);

    const sharedLock = lens.find((item) => item.codeHash === '0xaaa' && item.kind === 'lock');
    expect(sharedLock?.witnessIndex).toBe(0);
    expect(sharedLock?.inputIndices).toEqual([0, 1]);

    const typeGroup = lens.find((item) => item.codeHash === '0xbbb' && item.kind === 'type');
    expect(typeGroup?.witnessIndex).toBe(1);
    expect(typeGroup?.inputIndices).toEqual([1]);
  });

  it('infers witness coverage and extra witness payloads', () => {
    const tx = {
      inputsCount: 1,
      inputs: [{ lock: { codeHash: '0xaaa', hashType: 'type', args: '0x01' } }],
    };
    const witnessAnalyses = [
      analyzeWitness('0x1b00000010000000160000001600000006000000112205000000aa', 0, 1),
      analyzeWitness('0x64617301020304', 1, 1),
    ];
    const lens = buildScriptGroupLens(tx);

    const insights = inferWitnessInsights(tx, witnessAnalyses, lens);

    const coverageInsight = insights.find((insight) => insight.kind === 'input_witness_coverage');
    expect(coverageInsight).toBeDefined();
    expect(coverageInsight?.relatedWitnessIndices).toEqual([0]);
    expect(insights.some((insight) => insight.kind === 'extra_witnesses')).toBe(true);
    expect(insights.some((insight) => insight.kind === 'missing_input_witness')).toBe(false);
  });
});
