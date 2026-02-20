export function SiteFooter() {
  return (
    <footer className="border-t border-slate-800 bg-slate-950/90">
      <div className="container mx-auto flex flex-wrap items-center gap-3 px-4 py-3 font-mono text-xs text-slate-400">
        <span className="text-slate-500">Shortcuts</span>
        <span className="rounded border border-slate-700 px-2 py-0.5 text-slate-300">/ Search</span>
        <span className="rounded border border-slate-700 px-2 py-0.5 text-slate-300">
          Ctrl/Cmd+K Commands
        </span>
        <span className="rounded border border-slate-700 px-2 py-0.5 text-slate-300">? Help</span>
        <span className="rounded border border-slate-700 px-2 py-0.5 text-slate-300">
          g b Blocks
        </span>
        <span className="rounded border border-slate-700 px-2 py-0.5 text-slate-300">
          g t Transactions
        </span>
        <span className="rounded border border-slate-700 px-2 py-0.5 text-slate-300">g d DAO</span>
        <span className="rounded border border-slate-700 px-2 py-0.5 text-slate-300">
          g a Assets
        </span>
      </div>
    </footer>
  );
}
