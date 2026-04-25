'use client';

import { forwardRef } from 'react';

export interface ImageProps extends React.ImgHTMLAttributes<HTMLImageElement> {
  src: string;
  alt: string;
  width?: number;
  height?: number;
  fill?: boolean;
  priority?: boolean;
  unoptimized?: boolean;
}

const Image = forwardRef<HTMLImageElement, ImageProps>(function Image(
  { fill = false, style, width, height, priority: _priority, unoptimized: _unoptimized, ...props },
  ref
) {
  if (fill) {
    return (
      <img
        ref={ref}
        {...props}
        style={{
          position: 'absolute',
          inset: 0,
          width: '100%',
          height: '100%',
          objectFit: style?.objectFit ?? 'cover',
          ...style,
        }}
      />
    );
  }

  return <img ref={ref} {...props} width={width} height={height} style={style} />;
});

export default Image;
