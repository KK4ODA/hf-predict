// How a predicted reliability is described to the operator.

import { localHour, Zone } from "./localtime";

export type Tier = { key: "good" | "fair" | "poor" | "unlikely"; label: string; icon: string };

const GOOD: Tier = { key: "good", label: "Good", icon: "●" };
const FAIR: Tier = { key: "fair", label: "Fair", icon: "◐" };
const POOR: Tier = { key: "poor", label: "Poor", icon: "○" };
const UNLIKELY: Tier = { key: "unlikely", label: "Unlikely", icon: "–" };

/** Reliability at or above which a band counts as worth trying. */
export const FAIR_RELIABILITY = 0.3;
const GOOD_RELIABILITY = 0.7;
const POOR_RELIABILITY = 0.1;

export const TIER_LEGEND =
  "Good: 70% of days or more. Fair: 30–69%. Poor: 10–29%. Unlikely: under 10%.";

export function tierOf(reliability: number): Tier {
  if (reliability >= GOOD_RELIABILITY) return GOOD;
  if (reliability >= FAIR_RELIABILITY) return FAIR;
  if (reliability >= POOR_RELIABILITY) return POOR;
  return UNLIKELY;
}

/** Cell shading for a reliability: one hue, stronger with more. */
export function shade(reliability: number): string {
  return `rgba(57, 135, 229, ${(reliability * 0.7).toFixed(2)})`;
}

/** VOACAP numbers hours 1–24, where 24 is 00 UTC. */
export function clockHour(utcHour: number): number {
  return utcHour % 24;
}

export function hourLabel(utcHour: number): string {
  return String(clockHour(utcHour)).padStart(2, "0");
}

/**
 * Runs of consecutive UTC clock hours whose reliability is at least
 * `FAIR_RELIABILITY`, as [first, last] pairs. A run that crosses midnight is
 * joined; a whole open day is one run from 0 to 23.
 */
export function openRuns(byClockHour: number[]): [number, number][] {
  const open = byClockHour.map((r) => r >= FAIR_RELIABILITY);
  if (open.every(Boolean)) return [[0, 23]];
  const runs: [number, number][] = [];
  for (let h = 0; h < 24; h++) {
    if (!open[h]) continue;
    const last = runs[runs.length - 1];
    if (last && last[1] === h - 1) last[1] = h;
    else runs.push([h, h]);
  }
  if (runs.length > 1 && runs[0][0] === 0 && runs[runs.length - 1][1] === 23) {
    runs[0][0] = runs.pop()![0];
  }
  return runs;
}

/** The runs as "13–19, 22–01", in UTC when `zone` is null, else in local time. */
function runsText(runs: [number, number][], zone: Zone | null): string {
  const label = (h: number) => (zone ? localHour(h, zone) : String(h).padStart(2, "0"));
  return runs.map(([from, to]) => (from === to ? label(from) : `${label(from)}–${label(to)}`)).join(", ");
}

/** "13–19 UTC · 09–15 EDT", "all day", or null when no hour is worth trying. */
export function describeWindows(byClockHour: number[], zone: Zone): string | null {
  const runs = openRuns(byClockHour);
  if (runs.length === 0) return null;
  if (runs[0][0] === 0 && runs[0][1] === 23) return "all day";
  return `${runsText(runs, null)} UTC · ${runsText(runs, zone)} ${zone.name}`;
}
