import { describe, expect, it } from 'vitest';
import type { CurrentPattern } from '@/lib/api';
import { declaredStepGenes, stepGenes } from '@/lib/dob-declared';
import {
  headwearDescription,
  headwearInsights,
} from '@/__tests__/fixtures/objects/nervape-headwear';

type DobStep = Extract<CurrentPattern, { status: 'dob' }>['steps'][number];
const headwearStep = () => {
  const pattern = headwearInsights.currentPattern;
  if (pattern.status !== 'dob') throw new Error('fixture');
  return pattern.steps[0];
};

// Lane E follow-up: a DOB/0 cluster whose decoder binary the API has no reviewed byte mapping for
// (Nervape Headwear, 0x1c84…7b5f) still declares its pattern in its own description; the page
// reads that declaration — [name, type, offset, length, rule, args] — for that step only.
describe('declared DOB patterns', () => {
  it('reads the Headwear description’s pattern for its unmapped decoder step', () => {
    const step = headwearStep();
    expect(step.description.status).toBe('unsupported');
    const { source, genes } = stepGenes(step, headwearDescription, 'Nervape Headwear');
    expect(source).toBe('description');
    expect(
      genes.map((g) => [g.name, g.typeTag, g.byteOffset, g.byteLength, g.rule, g.ruleArguments])
    ).toEqual([
      ['prev.type', 'string', '0', '1', 'options', ['image']],
      ['prev.bg', 'string', '0', '74', 'utf8', null],
      ['prev.view', 'number', '74', '1', 'range', [0, 255]],
      ['Entity', 'number', '75', '8', 'raw', null],
    ]);
    expect(genes.map((g) => g.index)).toEqual([0, 1, 2, 3]);
  });

  it('keeps the API’s reviewed mapping for a supported step and waits for an unloaded description', () => {
    const supported: DobStep = {
      ...headwearStep(),
      description: {
        status: 'supported',
        format: 'jsonDob0',
        rangePolicy: 'clipToDna',
        genes: [
          {
            index: 0,
            traitIndex: 0,
            valueIndex: 0,
            name: 'Face',
            typeTag: 'String',
            byteOffset: '0',
            byteLength: '1',
            rule: 'options',
            ruleArguments: ['Red'],
          },
        ],
      },
    };
    expect(stepGenes(supported, null, 'x')).toMatchObject({
      source: 'decoder',
      genes: [{ name: 'Face' }],
    });
    // undefined: the cluster's description is not loaded yet; nothing is read, nothing thrown
    expect(stepGenes(headwearStep(), undefined, 'x')).toEqual({ source: 'none', genes: [] });
  });

  it('reads a DOB/1 decoder list by step and leaves a pattern of no byte ranges unread', () => {
    const description = JSON.stringify({
      dob: {
        ver: 1,
        decoders: [
          {
            decoder: { type: 'code_hash', hash: '0xaa' },
            pattern: [['name', 'String', 0, 40, 'utf8', []]],
          },
          {
            decoder: { type: 'code_hash', hash: '0xbb' },
            pattern: [['IMAGE.0', 'attributes', '', 'raw', "viewBox='0 0 500 500'"]],
          },
        ],
      },
    });
    const step = (n: number, hash: string): DobStep => ({
      ...headwearStep(),
      step: n,
      decoderReference: { type: 'code_hash', hash },
    });
    expect(declaredStepGenes(description, step(0, '0xAA'), 'x')!.map((g) => g.name)).toEqual([
      'name',
    ]);
    // the renderer's pattern composes an image; it declares no bytes
    expect(declaredStepGenes(description, step(1, '0xbb'), 'x')).toBeNull();
  });

  it('throws with context when the description cannot give an unmapped step its pattern', () => {
    const step = headwearStep();
    expect(() => stepGenes(step, null, 'Nervape Headwear')).toThrow(
      'Nervape Headwear: decoder step 1 (code_hash 0x1c84212ebd817e9de09d2a79f85cc421b684eda63409cfa75688f98716e77b5f) has no byte mapping and the cluster has no description to declare its pattern'
    );
    expect(() => stepGenes(step, '{"description":"plain"}', 'Nervape Headwear')).toThrow(
      'Nervape Headwear: decoder step 1 has no byte mapping and the cluster’s description declares no DOB pattern'
    );
    expect(() => stepGenes(step, 'not json', 'Nervape Headwear')).toThrow(
      'Nervape Headwear: the cluster’s description is not JSON'
    );
    const other = headwearDescription.replace('0x1c84212e', '0x00000000');
    expect(() => stepGenes(step, other, 'Nervape Headwear')).toThrow(
      'names decoder code_hash 0x00000000bd817e9de09d2a79f85cc421b684eda63409cfa75688f98716e77b5f, not the step’s'
    );
    const mixed = JSON.stringify({
      dob: {
        ver: 0,
        decoder: step.decoderReference,
        pattern: [
          ['a', 'string', 0, 1, 'utf8'],
          ['b', 'string', '', 'raw', 'x'],
        ],
      },
    });
    expect(() => stepGenes(step, mixed, 'Nervape Headwear')).toThrow(
      'Nervape Headwear: decoder step 1’s declared pattern mixes byte ranges with other rows (row 2)'
    );
  });
});
