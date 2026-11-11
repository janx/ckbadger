import type { ReactNode } from 'react';
import { PromptHeading } from '@/components/ui/prompt-heading';

/**
 * A tier-1 section of the network page (hierarchy R6): its heading a command line (rubbing × Song
 * print, phase 5) — the title as the command, what it counts as the args — 18 px above its
 * evidence; the section is named by its title alone.
 */
export function TopicSection({
  title,
  args,
  children,
}: {
  title: string;
  /** What the section counts once it is loaded: `66 nodes`, `90 days`. */
  args?: ReactNode;
  children: ReactNode;
}) {
  return (
    <section aria-label={title} className="min-w-0">
      <div className="mb-[18px] flex flex-wrap items-baseline justify-between gap-x-4 gap-y-1">
        <PromptHeading command={title} args={args} />
      </div>
      {children}
    </section>
  );
}
