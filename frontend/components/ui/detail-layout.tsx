import { createContext, useContext, type ReactNode } from 'react';
import { SealedHead, type SealedHeadOf } from '@/components/ui/page-seal';
import { PromptHeading } from '@/components/ui/prompt-heading';
import { cn } from '@/lib/utils';
import {
  documentTitleParts,
  useDocumentTitle,
  type DocumentTitleProps,
} from '@/lib/use-document-title';

/** Which information tier a section sits in; tier-2 headings are quieter (FINAL_REVIEW, TX-16). */
const DetailTierContext = createContext<1 | 2>(1);

/** F2 identities (round 2): which tier a section renders in, for headings outside DetailSection. */
export function useDetailTier(): 1 | 2 {
  return useContext(DetailTierContext);
}

/**
 * The developer-evidence tier (hierarchy plan R6): one visible boundary — 96 px, a hairline and the
 * "Evidence · for developers" eyebrow — and every section heading inside is quiet. `className`
 * lays out the tier's content (its sections), under the eyebrow. The hairline (Midnight, continued,
 * move 2; set flat in rubbing × Song print) is `.hairline-tube-border`, the 1 px `border-t` in the
 * line ink. The tier sits one step deeper (material pass v2, move 2): `.tier2-ground` bleeds
 * through the page gutters under the line, and the eyebrow is `.eyebrow` (500 at .12 em, move 3).
 */
export function DetailTier2({ children, className }: { children: ReactNode; className?: string }) {
  return (
    <DetailTierContext.Provider value={2}>
      <div
        data-tier="2"
        className="border-base-border hairline-tube-border tier2-ground mt-24 min-w-0 border-t pb-10 pt-7"
      >
        <p className="eyebrow text-meta text-text-dim">Evidence · for developers</p>
        <div className={cn('mt-7 min-w-0 space-y-8', className)}>{children}</div>
      </div>
    </DetailTierContext.Provider>
  );
}

export interface DetailFact {
  label: string;
  value: ReactNode;
}
interface DetailLayoutBaseProps {
  kind?: 'entity' | 'topic' | 'index';
  identity?: ReactNode;
  /**
   * The page's opening sentence, read with its name (rubbing × Song print, phase 5): the last line
   * of the head, after the identity. In a sealed head it is what is below the stone and the name
   * plate, from the page's left edge (the seals by kind, C10).
   */
  lede?: ReactNode;
  status?: ReactNode;
  facts?: readonly DetailFact[];
  children: ReactNode;
  /** Developer evidence. Omit on pages that are developer pages throughout. */
  tier2?: ReactNode;
  rawData?: ReactNode;
  rawOpen?: boolean;
  className?: string;
}
/**
 * Without a seal the head is the title row (the h1 and `status`, then `actions`), the identity and
 * the lede. With one it is the name plate (SealedHead, the seals by kind C10): the stone of
 * `seal.id` in its state, the kind line (`seal.kind`, the kind word, then `status`), the h1 and the
 * identity beside the stone, the lede below both. The plate has no place for actions.
 */
type DetailHeadProps =
  | { seal?: undefined; actions?: ReactNode }
  | { seal: SealedHeadOf; actions?: never };

/**
 * `title` is the h1; `documentTitle` (C1) names the browser tab when the h1 is not the page's name
 * or is not a plain string (entity pages: `[name, kind]`, e.g. `['#20,894,843', 'block']`).
 */
export type DetailLayoutProps = DetailLayoutBaseProps & DetailHeadProps & DocumentTitleProps;

/** Domain first, with one enforced boundary (DetailTier2) before developer evidence. */
export function DetailLayout({
  title,
  documentTitle,
  kind = 'entity',
  identity,
  seal,
  lede,
  status,
  actions,
  facts,
  children,
  tier2,
  rawData,
  rawOpen,
  className,
}: DetailLayoutProps) {
  useDocumentTitle(documentTitleParts(title, documentTitle));
  const h1 = (
    <h1
      // the weight (600) is h1.text-entity / h1.text-index in globals.css (move 3)
      className={cn(
        'text-text-bright break-words',
        kind === 'entity' ? 'text-entity' : 'text-index',
        // The name plate's name is one type step smaller below md (seals by kind, C10;
        // RESPONSIVE_LAYOUT principle 5): under the stone it has the full width, but at 30 px a
        // short out point (19 characters, 330.6 px) or a short address (21, 365.4 px) is wider than
        // the 328 px line at 360; at 26 px they are 286.5 and 316.7 px.
        seal && kind === 'entity' && 'max-md:text-index'
      )}
    >
      {title}
    </h1>
  );
  const head = seal ? (
    // The name plate: the kind word and the status on the kind line, the h1 and the identity
    // beside the stone, the lede under both from the page's left edge.
    <SealedHead {...seal} status={status} below={lede}>
      {h1}
      {identity && (
        <div className="text-text-dim text-small min-w-0 [overflow-wrap:anywhere]">{identity}</div>
      )}
    </SealedHead>
  ) : (
    <>
      <div className="flex flex-wrap items-baseline justify-between gap-3">
        <div className="flex min-w-0 flex-wrap items-baseline gap-3">
          {h1}
          {status}
        </div>
        {actions}
      </div>
      {identity && (
        <div className="text-text-dim text-small mt-2 min-w-0 [overflow-wrap:anywhere]">
          {identity}
        </div>
      )}
      {lede && (
        <div data-detail-lede="" className="mt-4 min-w-0">
          {lede}
        </div>
      )}
    </>
  );
  return (
    // `.arrive` (visual finish V7): the content fades in when the data arrives and this mounts.
    <div className={cn('arrive text-body min-w-0 font-mono', className)}>
      {/* An index page's title row closes on a hairline (Midnight, continued, move 2; BlocksAfter):
          h1 and legend, 14 px, the 1 px line, then the list. */}
      <header className={kind === 'index' ? 'mb-5' : 'mb-8'}>
        {/* The seal is pressed beside the name plate, never in the content (the facts and the
            sections run the full width under the head). */}
        {head}
        {facts && (
          // One column until md: at 640 a two-column grid wrapped labels into three lines.
          <dl className="mt-6 grid grid-cols-1 gap-x-12 gap-y-3 md:grid-cols-2">
            {facts.map(({ label, value }) => (
              <div
                key={label}
                className="grid min-w-0 grid-cols-1 items-baseline gap-x-3 gap-y-1 sm:grid-cols-[minmax(80px,1fr)_minmax(0,2fr)]"
              >
                <dt className="text-text-dim text-small min-w-0 break-words">{label}</dt>
                <dd className="text-text-bright min-w-0 break-words tabular-nums">{value}</dd>
              </div>
            ))}
          </dl>
        )}
        {kind === 'index' && <div aria-hidden="true" className="hairline-tube mt-3.5" />}
      </header>
      <div data-tier="1" className="min-w-0 space-y-8">
        {children}
      </div>
      {(tier2 || rawData) && (
        <DetailTier2>
          {tier2}
          {rawData && (
            <details open={rawOpen} className="border-base-border-subtle border-t pt-4">
              <summary className="text-text-dim hover:text-text text-small cursor-pointer">
                Raw data
              </summary>
              <div className="text-small mt-4 min-w-0 overflow-x-auto">{rawData}</div>
            </details>
          )}
        </DetailTier2>
      )}
    </div>
  );
}

export function DetailSection({
  title,
  args,
  actions,
  children,
  className,
}: {
  title: ReactNode;
  /**
   * What the command runs on (rubbing × Song print, phase 5): a count, a status, a name, in the
   * dim ink after the title — `cells 128 live`. Part of the heading's name in either tier.
   */
  args?: ReactNode;
  /** The row's right end: a note, a link, or the section's controls (a legend, a range). */
  actions?: ReactNode;
  children: ReactNode;
  className?: string;
}) {
  const tier = useContext(DetailTierContext);
  return (
    <section className={cn('min-w-0', className)}>
      <div className="mb-4 flex flex-wrap items-baseline justify-between gap-3">
        {/* A tier-1 h2 is a command line (rubbing × Song print, phase 2: the prompt square, the
            title as the command) at 600 through h2.text-section (globals.css, move 3); a tier-2
            h2 stays the quiet 500, unmarked, its args in the same dim ink. */}
        {tier === 2 ? (
          <h2 className="text-body text-text font-medium">
            {title}
            {args !== undefined && <span className="text-text-dim ml-2.5 font-normal">{args}</span>}
          </h2>
        ) : (
          <PromptHeading command={title} args={args} />
        )}
        {actions}
      </div>
      {children}
    </section>
  );
}
