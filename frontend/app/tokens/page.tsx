import { redirect } from 'next/navigation';

export default function TokensPage() {
  redirect('/assets?type=token');
}
