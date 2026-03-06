import { notFound } from 'next/navigation';

export const revalidate = 0;

export async function generateStaticParams() {
  return [];
}

export default function CatchAll() {
  notFound();
}
