import { useParams } from 'react-router-dom';
import { resolveDefaultNetwork } from '@/lib/runtime-config';

/** The active network from the `/:network` route segment, or the default. */
export function useActiveNetwork(): string {
  const { network } = useParams();
  return network || resolveDefaultNetwork();
}
