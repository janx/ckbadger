'use client';

interface ImageProps extends React.ImgHTMLAttributes<HTMLImageElement> {
  src: string;
  alt: string;
  width?: number;
  height?: number;
  fill?: boolean;
  priority?: boolean;
  unoptimized?: boolean;
}

export default function Image({
  fill = false,
  style,
  width,
  height,
  priority: _priority,
  unoptimized: _unoptimized,
  ...props
}: ImageProps) {
  if (fill) {
    return (
      <img
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

  return <img {...props} width={width} height={height} style={style} />;
}
