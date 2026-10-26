'use client';
import Link from '@/components/ui/link';

/** A failed read/render is a crash, not evidence that a chain entity does not exist. */
export function CrashPage({ message }: { message: string }) {
  return (
    <main className="mx-auto flex min-h-[75vh] max-w-4xl flex-col justify-center px-6 py-16 font-mono">
      <p className="text-text-dim text-[11px] uppercase tracking-[0.2em]">Explorer / interrupted</p>
      <h1
        className="text-text-bright my-6 text-[26px] font-medium"
        style={{ fontFamily: 'var(--font-error-display)' }}
      >
        Something went wrong
      </h1>
      <p className="text-text max-w-xl text-[13px]">
        The explorer could not complete this page. The chain continues.
      </p>
      <details open className="border-base-border-subtle mt-8 border-t pt-4">
        <summary className="text-text-dim cursor-pointer text-xs">Error details</summary>
        <pre role="alert" className="text-text mt-3 whitespace-pre-wrap break-words text-xs">
          {message}
        </pre>
      </details>
      <div className="mt-8 flex flex-wrap gap-6 text-xs">
        <button
          type="button"
          className="text-text-bright hover:text-jade"
          onClick={() => window.location.reload()}
        >
          Try again →
        </button>
        <Link href="/" className="text-text-dim hover:text-jade">
          Back to the explorer →
        </Link>
      </div>
    </main>
  );
}
