import type { ReactNode } from 'react';

/**
 * A tier-1 section of the network page (hierarchy R6): its h2 at section size, 18 px above its
 * evidence; the section is named by its title.
 */
export function TopicSection({ title, children }: { title: string; children: ReactNode }) {
  return (
    <section aria-label={title} className="min-w-0">
      <div className="mb-[18px] flex flex-wrap items-baseline justify-between gap-x-4 gap-y-1">
        <h2 className="text-text-bright text-section font-medium">{title}</h2>
      </div>
      {children}
    </section>
  );
}
