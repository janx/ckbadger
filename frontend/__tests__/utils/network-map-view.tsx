import { NetworkMapFootnotes, NetworkMapPicture, useNetworkMap } from '@/app/network/network-map';

/**
 * The network map's picture and its footnotes on one map state, as the network page composes them
 * (app/network/client-page.tsx splits them across its hero and body); the map tests render the
 * two together. Lane E3 moved it here: the app had no other user of it.
 */
export function NetworkMapView({ neverVerified }: { neverVerified: number }) {
  const map = useNetworkMap(neverVerified);
  return (
    <div className="min-w-0">
      <NetworkMapPicture map={map} />
      <NetworkMapFootnotes map={map} />
    </div>
  );
}
