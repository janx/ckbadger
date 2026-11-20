import { createContext, useContext } from 'react';

/**
 * Which information tier a section sits in; tier-2 headings are quieter (FINAL_REVIEW, TX-16).
 * Its own module (lane cd-fixes-controls, the disclosure sweep) so a library component that reads
 * its tier (SectionDisclosure) and the layout that sets it (components/ui/detail-layout.tsx, which
 * draws a SectionDisclosure) do not import each other.
 */
export const DetailTierContext = createContext<1 | 2>(1);

/** F2 identities (round 2): which tier a section renders in, for headings outside DetailSection. */
export function useDetailTier(): 1 | 2 {
  return useContext(DetailTierContext);
}
