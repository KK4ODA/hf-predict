// Shared marks and status pieces, so every view draws the same quantity the
// same way: reliability in the model blue steps, what was heard in amber,
// status as a dot plus a word.

import { CSSProperties, ReactNode, RefObject, useEffect, useRef, useState } from "react";
import { BandComparison } from "./types";

/** Upper edges of the reliability steps: they match the tier thresholds. */
const REL_EDGES = [0.1, 0.3, 0.5, 0.7, 0.9];
const REL_LABELS = ["0", "10", "30", "50", "70", "90"];

/** Which of the six reliability steps a value falls in, 0 to 5. */
export function relBin(reliability: number): number {
  let bin = 0;
  while (bin < REL_EDGES.length && reliability >= REL_EDGES[bin]) bin++;
  return bin;
}

/** Fill and readable ink for a cell shaded by reliability. */
export function relStyle(reliability: number): CSSProperties {
  const bin = relBin(reliability);
  return { background: `var(--rel-${bin})`, color: `var(--rel-ink-${bin})` };
}

/** The same for an SVG mark. */
export function relFill(reliability: number): CSSProperties {
  return { fill: `var(--rel-${relBin(reliability)})` };
}

export const pct = (share: number) => `${Math.round(share * 100)}%`;

/** The reliability scale, as a key. */
export function RelScale({ label = "Predicted reliability" }: { label?: string }) {
  return (
    <span className="legend-item" style={{ display: "inline-flex", alignItems: "center", gap: 8 }}>
      <span className="hint">{label}</span>
      <span className="relscale" aria-hidden="true">
        {REL_LABELS.map((_, i) => (
          <span key={i} style={{ background: `var(--rel-${i})` }} />
        ))}
        {REL_LABELS.map((l, i) => (
          <small key={`l${i}`}>{l}</small>
        ))}
      </span>
      <span className="hint">%</span>
    </span>
  );
}

/** Reliability for each UTC hour, 0 to 23, as a row of small cells. */
export function HourStrip({
  byClockHour,
  selected,
  now,
  label,
}: {
  byClockHour: number[];
  selected?: number;
  now?: number;
  label: string;
}) {
  return (
    <span className="strip" role="img" aria-label={label} title={label}>
      {byClockHour.map((r, h) => (
        <span
          key={h}
          className={[h === selected ? "sel" : "", h === now ? "now" : ""].join(" ").trim() || undefined}
          style={{ background: `var(--rel-${relBin(r)})` }}
        />
      ))}
    </span>
  );
}

/** A 0-100 % value as a bar, with ticks at the fair and good thresholds. */
export function Meter({
  value,
  tone = "model",
  label,
  ticks = [0.3, 0.7],
}: {
  value: number;
  tone?: "model" | "meas";
  label?: ReactNode;
  ticks?: number[];
}) {
  const share = Math.max(0, Math.min(1, value));
  return (
    <span className={`meter ${tone}`}>
      {label !== undefined && <span className="lbl">{label}</span>}
      <span className="track" aria-hidden="true">
        <span className="fill" style={{ width: `${share * 100}%` }} />
        {ticks.map((t) => (
          <span key={t} className="tick" style={{ left: `${t * 100}%` }} />
        ))}
      </span>
      <span className="value">{pct(value)}</span>
    </span>
  );
}

export const OBSERVED_WORDS: Record<BandComparison["observed"], string> = {
  notSampled: "Not listened to",
  none: "Nothing heard that way",
  limited: "Limited",
  moderate: "Moderate",
  strong: "Strong",
};

/** How much was heard toward a destination, in three steps. */
export function Evidence({ tier }: { tier: BandComparison["observed"] }) {
  return (
    <span className={`evidence ${tier}`} role="img" aria-label={OBSERVED_WORDS[tier]}>
      <span />
      <span />
      <span />
    </span>
  );
}

export type Health = "ok" | "warn" | "alert" | "off" | "busy";

export function Dot({ state }: { state: Health }) {
  return <span className={`dot ${state}`} aria-hidden="true" />;
}

export function Pill({ state, children }: { state: Health; children: ReactNode }) {
  return (
    <span className="pill">
      <Dot state={state} />
      {children}
    </span>
  );
}

/** A duration in plain words: 40 s, 12 min, 3 h, 2 days. */
const MODE_NAMES: Record<string, string> = { PKTUSB: "DATA-U", PKTLSB: "DATA-L", PKTFM: "DATA-FM", PKTAM: "DATA-AM" };

/** Hamlib's mode names as radios show them: PKTUSB is DATA-U. */
export const modeName = (mode: string) => MODE_NAMES[mode] ?? mode;

export function age(seconds: number): string {
  if (seconds < 90) return `${Math.max(0, Math.round(seconds))} s`;
  if (seconds < 90 * 60) return `${Math.round(seconds / 60)} min`;
  if (seconds < 48 * 3600) return `${Math.round(seconds / 3600)} h`;
  return `${Math.round(seconds / 86400)} days`;
}

/** The width of an element, followed as the window resizes, so charts draw at 1:1. */
export function useWidth<T extends HTMLElement>(fallback: number): [RefObject<T | null>, number] {
  const ref = useRef<T>(null);
  const [width, setWidth] = useState(fallback);
  useEffect(() => {
    const element = ref.current;
    if (!element || typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver(([entry]) => setWidth(Math.round(entry.contentRect.width)));
    observer.observe(element);
    return () => observer.disconnect();
  }, []);
  return [ref, width];
}
