import { hexBytes } from '@/lib/hex-bytes';
import { CLASS_COLORS } from '@/lib/design-tokens';
/** A compact sixteen-lane gel: every nibble is drawn in its actual hexadecimal lane. */
export function DnaStrip({
  dna,
  label = 'Object DNA',
  color = CLASS_COLORS.objects,
}: {
  dna: string;
  label?: string;
  color?: string;
}) {
  const bytes = hexBytes(dna, label);
  const digits = bytes.join('').split('');
  const width = 160,
    height = 32,
    pitch = digits.length ? width / digits.length : 0;
  return (
    <svg
      role="img"
      aria-label={`${label}: ${bytes.length} bytes, ${bytes.join('')}`}
      viewBox={`0 0 ${width} ${height}`}
      width={width}
      height={height}
      className="max-w-full"
    >
      {Array.from({ length: 16 }, (_, lane) => (
        <line
          key={lane}
          x1="0"
          x2={width}
          y1={lane * 2 + 1}
          y2={lane * 2 + 1}
          stroke={color}
          strokeOpacity=".08"
        />
      ))}
      {digits.map((digit, i) => (
        <rect
          key={i}
          data-nibble={i}
          x={i * pitch}
          y={parseInt(digit, 16) * 2}
          width={pitch * 0.7}
          height="1.5"
          fill={color}
        />
      ))}
    </svg>
  );
}
