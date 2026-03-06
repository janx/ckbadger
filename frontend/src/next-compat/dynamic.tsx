'use client';

import { ComponentType, Suspense, lazy } from 'react';

type Loader<TProps> = () => Promise<{ default: ComponentType<TProps> } | ComponentType<TProps>>;

interface DynamicOptions<TProps> {
  loading?: ComponentType<TProps>;
  ssr?: boolean;
}

export default function dynamic<TProps extends object>(
  loader: Loader<TProps>,
  options?: DynamicOptions<TProps>
) {
  const LazyComponent = lazy(async () => {
    const loaded = await loader();
    if ('default' in loaded) {
      return loaded;
    }
    return { default: loaded };
  });

  const LoadingComponent = options?.loading;

  return function DynamicComponent(props: TProps) {
    const fallback = LoadingComponent ? <LoadingComponent {...props} /> : null;
    return (
      <Suspense fallback={fallback}>
        <LazyComponent {...props} />
      </Suspense>
    );
  };
}
