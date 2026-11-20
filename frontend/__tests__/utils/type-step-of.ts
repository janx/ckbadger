import { TYPE_STEPS, type TypeStep } from '@/lib/type-scale';

/**
 * The type step an element's words are set at, as its classes say (jsdom applies no stylesheet):
 * its own `text-<step>` with no variant, else the nearest ancestor's; `none` where nothing sets one.
 * The size half of a word's role (the ink half is `inkOf`): a value, a link, a table cell and a
 * sentence at 13 px or more, a label, a unit, a column head and a hint at the meta step.
 */
export function typeStepOf(el: Element): TypeStep | 'none' {
  for (let e: Element | null = el; e; e = e.parentElement)
    for (const c of Array.from(e.classList)) {
      const step = c.startsWith('text-') ? c.slice('text-'.length) : null;
      if (step !== null && (TYPE_STEPS as readonly string[]).includes(step))
        return step as TypeStep;
    }
  return 'none';
}
