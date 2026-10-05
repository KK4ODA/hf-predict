import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { BandComparison, PathDetail } from "./types";

const POLL_MS = 5000;

export const OBSERVED: Record<BandComparison["observed"], string> = {
  notSampled: "not listened to",
  none: "nothing heard",
  limited: "limited",
  moderate: "moderate",
  strong: "strong",
};

/** FT8's required SNR in dB-Hz: its -21 dB decode threshold in 2500 Hz. */
const FT8_REQUIRED_SNR = 13;
/** Below this the operator's own mode is unlikely to get through. */
const MODE_POOR = 0.3;
/** Recommendations at or above this rank say the band is worth trying. */
const WORTH_TRYING = 4;

/**
 * The recommendation rests on FT8. When the operator's mode is something
 * else and is itself predicted poor, say so beside any encouraging label.
 */
export function modeCaution(
  row: BandComparison,
  modeLabel: string,
  requiredSnrDbHz: number,
): string | null {
  const extraDb = requiredSnrDbHz - FT8_REQUIRED_SNR;
  if (extraDb <= 0 || row.verdict.priority > WORTH_TRYING || row.modeReliability >= MODE_POOR) {
    return null;
  }
  return (
    `That is for FT8. ${modeLabel} needs about ${extraDb.toFixed(0)} dB more signal and is ` +
    `predicted on only ${(row.modeReliability * 100).toFixed(0)}% of days.`
  );
}

/** A symbol for each recommendation, so colour is never the only cue. */
export function verdictIcon(priority: number): string {
  if (priority <= 2) return "●";
  if (priority === 3) return "▲";
  if (priority === 4) return "◐";
  if (priority <= 6) return "○";
  return "–";
}

/**
 * The path's predictions for one hour beside what has been heard toward the
 * destination in the last `minutes`, refreshed as decodes arrive.
 */
export function useComparison(
  detail: PathDetail,
  hourIndex: number,
  minutes: number,
): [BandComparison[], string] {
  const [rows, setRows] = useState<BandComparison[]>([]);
  const [error, setError] = useState("");

  useEffect(() => {
    const prediction = detail.prediction;
    const hour = prediction.run.prediction.hours[hourIndex];
    const query = {
      minutes,
      destination: prediction.rx,
      pathBearingDeg: prediction.txBearingDeg,
      pathDistanceKm: prediction.distanceKm,
      bands: prediction.bands.map((band, i) => ({
        name: band.name,
        modeReliability: hour.frequencies[i].reliability,
        ft8Reliability: detail.ft8Reliability[hourIndex][i],
      })),
    };
    let current = true;
    const poll = () =>
      invoke<BandComparison[]>("compare_path", { query })
        .then((next) => {
          if (!current) return;
          setRows(next);
          setError("");
        })
        .catch((e) => current && setError(String(e)));
    poll();
    const timer = setInterval(poll, POLL_MS);
    return () => {
      current = false;
      clearInterval(timer);
    };
  }, [detail, hourIndex, minutes]);

  return [rows, error];
}
