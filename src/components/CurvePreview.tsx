import type { CurvePreview as CurvePreviewData } from "../bindings";

const W = 320;
const H = 140;
const PAD = 28;
const LINES = [
  { key: "slow", name: "Slow tick", stroke: "stroke-sky-500" },
  { key: "fast", name: "Fast tick", stroke: "stroke-orange-500" },
] as const;

type Point = [number, number];

/** specta types f64 as `number | null`; Rust never sends null here, so read it as 0. */
function points(raw: CurvePreviewData["slow"]): Point[] {
  return raw.map(([ms, px]) => [ms ?? 0, px ?? 0]);
}

function end(line: Point[]): Point {
  return line[line.length - 1] ?? [0, 0];
}

/** Pixels against milliseconds for one slow and one fast tick. Static: nothing animates. */
export function CurvePreview({ preview }: { preview: CurvePreviewData }) {
  const lines = { slow: points(preview.slow), fast: points(preview.fast) };
  const all = [...lines.slow, ...lines.fast];
  const maxMs = Math.max(1, ...all.map(([ms]) => ms));
  const maxPx = Math.max(1, ...all.map(([, px]) => px));
  const x = (ms: number) => PAD + (ms / maxMs) * (W - PAD - 8);
  const y = (px: number) => H - PAD - (px / maxPx) * (H - PAD - 8);
  const path = (line: Point[]) =>
    line.map(([ms, px], i) => `${i === 0 ? "M" : "L"}${x(ms).toFixed(1)},${y(px).toFixed(1)}`).join(" ");
  const summary = LINES.map(({ key, name }) => {
    const [ms, px] = end(lines[key]);
    return `${name}: ${Math.round(px)} px over ${Math.round(ms)} ms`;
  }).join("; ");

  return (
    <figure className="mb-6">
      <svg role="img" aria-label={`Scroll curve. ${summary}.`} viewBox={`0 0 ${W} ${H}`} className="w-full max-w-sm">
        <line x1={PAD} y1={H - PAD} x2={W - 8} y2={H - PAD} className="stroke-neutral-400" />
        <line x1={PAD} y1={8} x2={PAD} y2={H - PAD} className="stroke-neutral-400" />
        <text x={W - 8} y={H - 8} textAnchor="end" className="fill-neutral-500 text-[10px]">
          {Math.round(maxMs)} ms
        </text>
        <text x={4} y={14} className="fill-neutral-500 text-[10px]">
          {Math.round(maxPx)} px
        </text>
        {LINES.map(({ key, stroke }) => (
          <path key={key} d={path(lines[key])} fill="none" strokeWidth={2} className={stroke} />
        ))}
      </svg>
      <figcaption className="mt-1 flex gap-4 text-xs text-neutral-500">
        {LINES.map(({ key, name, stroke }) => (
          <span key={key} className="flex items-center gap-1">
            <svg width="12" height="2" aria-hidden="true">
              <line x1="0" y1="1" x2="12" y2="1" strokeWidth={2} className={stroke} />
            </svg>
            {name}
          </span>
        ))}
      </figcaption>
    </figure>
  );
}
