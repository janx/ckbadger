/**
 * The controls library's proof page (UI review 2026-11-14, phase C, lane C1; proof/controls.html):
 * every control of components/ui/ in each state the controls board shows (gate D0′, passed
 * 2026-11-19), by night (the page's own theme) and by day (the day's declarations of
 * lib/palette.ts on its section), in the app's `container` with its gutters and the app's
 * stylesheet, so the controls can be measured in a real browser at 1280, 1024, 640, 390 and 360
 * before any page uses them. Development only: the dev server serves it, `vite build` never reads
 * it. Each control is live: a click, a key or a narrower window shows what it does there (the tab
 * row's counts leave at 360, more than four filter values become a select below 640).
 */
import { StrictMode, useState, type CSSProperties, type ReactNode } from 'react';
import ReactDOM from 'react-dom/client';
import { CopyButton } from '@/components/ui/copy-button';
import { FilterChips, type FilterValue } from '@/components/ui/filter-chips';
import { HelpPopover } from '@/components/ui/help-popover';
import { Pager } from '@/components/ui/pager';
import { RangeSelect } from '@/components/ui/range-select';
import { Select } from '@/components/ui/select';
import { Tabs, type Tab } from '@/components/ui/tabs';
import { TextField } from '@/components/ui/text-field';
import { themeDeclarations } from '@/lib/palette';
import '@/app/globals.css';

/** One control of the board, its name in the quiet ink beside it, its states under one another. */
function Item({ name, children }: { name: string; children: ReactNode }) {
  return (
    <div className="border-row-rule grid gap-x-6 gap-y-3 border-t py-4 sm:grid-cols-[120px_minmax(0,1fr)]">
      <span className="text-meta text-text-dim pt-1.5">{name}</span>
      <div className="flex min-w-0 flex-col gap-4">{children}</div>
    </div>
  );
}

/** A note under a state: what it shows, in the quiet ink. */
function Note({ children }: { children: ReactNode }) {
  return <span className="text-meta text-text-dim">{children}</span>;
}

type AddressTab = 'activities' | 'transactions' | 'cells';
const ADDRESS_TABS: Tab<AddressTab>[] = [
  { key: 'activities', label: 'activities', count: 3 },
  { key: 'transactions', label: 'transactions', count: 3 },
  { key: 'cells', label: 'live cells', count: 3 },
];
type InventoryTab = 'tokens' | 'objects' | 'identities';
const INVENTORY_TABS: Tab<InventoryTab>[] = [
  { key: 'tokens', label: 'tokens', count: 727 },
  { key: 'objects', label: 'objects', count: 5653 },
  { key: 'identities', label: 'identities', count: 4 },
];

function TabRows() {
  const [address, setAddress] = useState<AddressTab>('activities');
  const [inventory, setInventory] = useState<InventoryTab>('tokens');
  const [plain, setPlain] = useState<AddressTab>('transactions');
  return (
    <Item name="tab row">
      <Tabs label="address" command tabs={ADDRESS_TABS} value={address} onChange={setAddress}>
        <Note>the {address} panel · 356.4 px with its counts: they leave the row at 360</Note>
      </Tabs>
      <Tabs
        label="inventory"
        command
        tabs={INVENTORY_TABS}
        value={inventory}
        onChange={setInventory}
      >
        <Note>the {inventory} panel · 324 px with its counts: they stay at 360</Note>
      </Tabs>
      <Tabs
        label="a tab row that is not a command line, with a zero"
        tabs={[ADDRESS_TABS[0], { ...ADDRESS_TABS[1], count: 0 }, ADDRESS_TABS[2]]}
        value={plain}
        onChange={setPlain}
      >
        <Note>no square, the names in their own case; a zero’s name in the quiet ink</Note>
      </Tabs>
    </Item>
  );
}

type Kind = 'all' | 'ckb' | 'scripts' | 'dao' | 'tokens' | 'objects';
const KINDS: FilterValue<Kind>[] = [
  { key: 'all', label: 'all', count: 6122 },
  { key: 'ckb', label: 'ckb transfers', count: 683, mark: 'jade' },
  { key: 'scripts', label: 'script calls', count: 5425, mark: 'slate-ckb' },
  { key: 'dao', label: 'nervos dao', count: 11, mark: 'gold' },
  { key: 'tokens', label: 'tokens', count: 3, mark: 'token' },
  { key: 'objects', label: 'objects', count: 0, mark: 'object' },
];
type Standard = 'all' | 'xudt' | 'sudt' | 'compatible';
const STANDARDS: FilterValue<Standard>[] = [
  { key: 'all', label: 'all', count: 727 },
  { key: 'xudt', label: 'xudt', count: 629 },
  { key: 'sudt', label: 'sudt', count: 94 },
  { key: 'compatible', label: 'xudt-compatible', count: 4 },
];

function Filters() {
  const [kind, setKind] = useState<Kind>('all');
  const [standard, setStandard] = useState<Standard>('all');
  return (
    <Item name="filter chips">
      <FilterChips label="kind" values={KINDS} value={kind} onChange={setKind} />
      <Note>six values: chips from 640, the select after its label below it</Note>
      <FilterChips label="standard" values={STANDARDS} value={standard} onChange={setStandard} />
      <Note>four values: chips at every width</Note>
    </Item>
  );
}

const WEEKS = ['1 w', '1 m', '1 y', 'all'];
const DAYS = ['30 d', '90 d', '1 y', 'all'];
const HOURS = ['24 h', '14 d'];
const spans = (labels: string[]) => labels.map((label) => ({ key: label, label }));

function Ranges() {
  const [weeks, setWeeks] = useState('all');
  const [days, setDays] = useState('90 d');
  const [hours, setHours] = useState('24 h');
  return (
    <Item name="range">
      <div className="flex flex-wrap items-center gap-2.5">
        <RangeSelect label="range" options={spans(WEEKS)} value={weeks} onChange={setWeeks} />
        <RangeSelect label="range" options={spans(DAYS)} value={days} onChange={setDays} />
        <RangeSelect label="range" options={spans(HOURS)} value={hours} onChange={setHours} />
      </div>
    </Item>
  );
}

function Pagers() {
  const [page, setPage] = useState(3);
  const go = (to: number) => () => setPage(to);
  return (
    <Item name="pager">
      <Pager
        mode="pages"
        label="tokens"
        from={(page - 1) * 20 + 1}
        to={Math.min(page * 20, 727)}
        total={727}
        pageSize={20}
        page={page}
        pages={37}
        onPrevious={go(page - 1)}
        onNext={go(page + 1)}
      />
      <Note>pages: a ranked list (live: prev and next walk its 37 pages)</Note>
      <Pager
        mode="time"
        label="blocks"
        order="newest first"
        page={1}
        pages={2651295}
        onOlder={() => {}}
      />
      <Note>a cursor in time at its newest end: newer cannot be taken</Note>
      <Pager
        mode="more"
        all={{ count: 8, singular: 'branch switch', plural: 'branch switches', onShow: () => {} }}
        next={{ count: 50, onShow: () => {} }}
      />
      <Note>the short list’s own forms</Note>
      <Pager
        mode="pages"
        label="one page"
        from={1}
        to={8}
        total={8}
        pageSize={20}
        page={1}
        pages={1}
        onPrevious={() => {}}
        onNext={() => {}}
      />
      <Note>one page: nothing is drawn above this line</Note>
    </Item>
  );
}

const SORTS = [
  { value: 'capacity', label: 'ckb held in its cells' },
  { value: 'holders', label: 'holders' },
  { value: 'name', label: 'name' },
] as const;
type Sort = (typeof SORTS)[number]['value'];

function Fields() {
  const [name, setName] = useState('');
  const [country, setCountry] = useState('');
  const [sort, setSort] = useState<Sort>('capacity');
  return (
    <Item name="field, select">
      <TextField
        label="search the inventory"
        search
        placeholder="search by name"
        value={name}
        onChange={setName}
        className="w-[200px]"
      />
      <TextField
        label="country"
        placeholder="country"
        value={country}
        onChange={setCountry}
        className="w-[200px]"
      />
      <Select label="sorted by" value={sort} options={SORTS} onChange={setSort} />
    </Item>
  );
}

function CopyAndHelp() {
  return (
    <Item name="copy, help">
      <span className="text-small text-text flex flex-wrap items-center gap-2.5">
        0x495ef701…31cf4a
        <CopyButton value={`0x495ef701${'0'.repeat(48)}31cf4a`} label="copy transaction hash" />
        <HelpPopover label="Explain HMul" title="HMul">
          <p className="max-w-measure">CKB held in its cells ÷ occupied.</p>
        </HelpPopover>
      </span>
      <Note>
        copied for 1.5 s after a copy; not copied where the clipboard refuses (a page over plain
        HTTP on a LAN address)
      </Note>
    </Item>
  );
}

function Board({ theme }: { theme: string }) {
  return (
    <section className="py-8">
      <p className="text-meta text-text-dim pb-3 uppercase tracking-[0.12em]">{theme}</p>
      <TabRows />
      <Filters />
      <Ranges />
      <Pagers />
      <Fields />
      <CopyAndHelp />
    </section>
  );
}

function ProofPage() {
  return (
    <>
      <main className="bg-base-bg text-text">
        <div className="container mx-auto">
          <Board theme="by night" />
        </div>
      </main>
      <div className="bg-base-bg text-text" style={themeDeclarations('light') as CSSProperties}>
        <div className="container mx-auto">
          <Board theme="by day" />
        </div>
      </div>
    </>
  );
}

const rootElement = document.getElementById('root');
if (!rootElement) throw new Error('proof/controls.html: no #root to mount the controls on');
ReactDOM.createRoot(rootElement).render(
  <StrictMode>
    <ProofPage />
  </StrictMode>
);
