import { redirect } from '@/src/navigation';

export default function TokensPage() {
  redirect('/assets?type=token');
}
