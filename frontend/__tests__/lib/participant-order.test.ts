import { describe, expect, it } from 'vitest';
import { orderParticipants, participantsToShow, PARTICIPANT_LIMIT } from '@/lib/participant-order';

const p = (id: string, ckbDelta: string) => ({ id, ckbDelta });
const ids = (list: { id: string }[]) => list.map((x) => x.id);

describe('orderParticipants', () => {
  it('lists what was paid first, then what was received, then the zeros — each largest first', () => {
    const list = [
      p('zero-a', '0'),
      p('in-small', '100'),
      p('out-small', '-5'),
      p('in-big', '900000000000'),
      p('out-big', '-900000000001'),
      p('zero-b', '0'),
    ];
    expect(ids(orderParticipants(list))).toEqual([
      'out-big',
      'out-small',
      'in-big',
      'in-small',
      'zero-a',
      'zero-b',
    ]);
  });

  it('compares exact shannons, not floats', () => {
    const list = [p('a', '-9007199254740993'), p('b', '-9007199254740992')];
    expect(ids(orderParticipants(list))).toEqual(['a', 'b']);
  });

  it('keeps the API order among equal amounts and does not mutate its input', () => {
    const list = [p('first', '-10'), p('second', '-10')];
    expect(ids(orderParticipants(list))).toEqual(['first', 'second']);
    expect(ids(list)).toEqual(['first', 'second']);
  });
});

describe('participantsToShow', () => {
  it(`shows every participant up to ${PARTICIPANT_LIMIT}`, () => {
    const list = Array.from({ length: 6 }, (_, i) => p(`p${i}`, String(-i)));
    expect(participantsToShow(list, false)).toEqual({ shown: orderParticipants(list), hidden: 0 });
  });

  it('over six, shows the five largest |CKB delta| in display order and counts the rest', () => {
    const list = [
      p('in-3', '300'),
      p('zero', '0'),
      p('out-1', '-100'),
      p('in-9', '900'),
      p('out-7', '-700'),
      p('in-2', '200'),
      p('out-5', '-500'),
    ];
    const view = participantsToShow(list, false);
    // The five largest are 900, 700, 500, 300, 200; shown in the negative-first order.
    expect(ids(view.shown)).toEqual(['out-7', 'out-5', 'in-9', 'in-3', 'in-2']);
    expect(view.hidden).toBe(2);
  });

  it('expanded, shows them all in display order', () => {
    const list = Array.from({ length: 8 }, (_, i) => p(`p${i}`, String(i % 2 ? i : -i)));
    expect(participantsToShow(list, true)).toEqual({ shown: orderParticipants(list), hidden: 0 });
  });
});
