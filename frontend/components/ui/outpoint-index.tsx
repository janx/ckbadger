/**
 * An outpoint's `:index`, after its tx hash: one unit that never breaks inside (":1 / 6"), even in
 * a box that lets the hash itself break anywhere. `display: inline` is the span's default, stated
 * so that a name computed without a stylesheet still reads "hash:1", as browsers read it.
 */
export function OutpointIndex({ index }: { index: number | string }) {
  return (
    <span data-outpoint-index className="whitespace-nowrap" style={{ display: 'inline' }}>
      :{index}
    </span>
  );
}
