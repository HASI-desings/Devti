import { encode } from "uqr";

/**
 * QR code drawn as soft round modules with rounded finder eyes. Always dark-on-light
 * (on its own light card) so every phone camera can read it in either theme.
 */
export function Qr({ text, size = 220, label }: { text: string; size?: number; label: string }) {
  const qr = encode(text, { ecc: "M", border: 0 });
  const n = qr.size;
  const inFinder = (x: number, y: number) =>
    (x < 7 && y < 7) || (x >= n - 7 && y < 7) || (x < 7 && y >= n - 7);
  const modules: { x: number; y: number }[] = [];
  qr.data.forEach((row, y) => {
    row.forEach((on, x) => {
      if (on && !inFinder(x, y)) modules.push({ x, y });
    });
  });
  const eyes = [
    [0, 0],
    [n - 7, 0],
    [0, n - 7],
  ];
  return (
    <svg
      width={size}
      height={size}
      viewBox={`-2 -2 ${n + 4} ${n + 4}`}
      role="img"
      aria-label={label}
      data-qr-size={n}
      style={{ display: "block" }}
    >
      <rect x="-2" y="-2" width={n + 4} height={n + 4} rx="3" fill="#fbfaff" />
      <g fill="#17142b">
        {modules.map(({ x, y }) => (
          <circle key={`${x}.${y}`} cx={x + 0.5} cy={y + 0.5} r={0.46} />
        ))}
      </g>
      {eyes.map(([x, y]) => (
        <g key={`${x}-${y}`}>
          <rect
            x={x! + 0.5}
            y={y! + 0.5}
            width={6}
            height={6}
            rx={1.8}
            fill="none"
            stroke="#17142b"
            strokeWidth={1}
          />
          <rect x={x! + 2} y={y! + 2} width={3} height={3} rx={0.9} fill="#17142b" />
        </g>
      ))}
    </svg>
  );
}
