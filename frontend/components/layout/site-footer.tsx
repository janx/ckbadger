export function SiteFooter() {
  return (
    <footer className="border-t border-slate-800 bg-slate-950/90">
      <div className="container mx-auto flex flex-wrap items-end gap-3 px-4 py-3 font-mono text-xs text-slate-400">
        <div className="flex items-center gap-3">
          <a
            href="https://github.com/janx/ckbadger"
            target="_blank"
            rel="noreferrer"
            className="text-terminal-green hover:underline"
          >
            Github
          </a>
          <span className="text-slate-600">|</span>
          <span className="text-slate-500">Press ? for shortcuts</span>
        </div>
        <span className="ml-auto text-right">
          Built by{' '}
          <a
            href="https://x.com/busyforking"
            target="_blank"
            rel="noreferrer"
            className="text-terminal-green hover:underline"
          >
            @busyforking
          </a>{' '}
          with agents Coco and Dede ❤️
        </span>
      </div>
    </footer>
  );
}
