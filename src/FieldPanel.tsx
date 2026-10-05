import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { modeCaution, OBSERVED, useComparison, verdictIcon } from "./comparison";
import { WorldMap } from "./WorldMap";
import { Conditions, Observation, PathDetail } from "./types";
import { clockHour, hourLabel, openWindows } from "./tiers";

const EVIDENCE_MINUTES = 60;
const TOP_BANDS = 3;
const POLL_MS = 5000;

function age(seconds: number): string {
  if (seconds < 90) return `${Math.max(0, Math.round(seconds))} s`;
  if (seconds < 90 * 60) return `${Math.round(seconds / 60)} min`;
  if (seconds < 48 * 3600) return `${Math.round(seconds / 3600)} h`;
  return `${Math.round(seconds / 86400)} days`;
}

/** The newest stored decode, refreshed while the screen is open. */
function useLastDecode(): Observation | null {
  const [last, setLast] = useState<Observation | null>(null);
  useEffect(() => {
    let current = true;
    const poll = () =>
      invoke<Observation[]>("recent_observations", { limit: 1 })
        .then((recent) => current && setLast(recent[0] ?? null))
        .catch(() => current && setLast(null));
    poll();
    const timer = setInterval(poll, POLL_MS);
    return () => {
      current = false;
      clearInterval(timer);
    };
  }, []);
  return last;
}

type Props = {
  detail: PathDetail;
  hourIndex: number;
  modeLabel: string;
  longPath: boolean;
  month: number;
  conditions: Conditions | null;
};

/** One simplified screen for working a path: what to try, when, and how fresh the data is. */
export function FieldPanel({ detail, hourIndex, modeLabel, longPath, month, conditions }: Props) {
  const [rows, error] = useComparison(detail, hourIndex, EVIDENCE_MINUTES);
  const last = useLastDecode();
  const prediction = detail.prediction;
  const hours = prediction.run.prediction.hours;
  const hour = hours[hourIndex];
  const now = Date.now() / 1000;

  const ranked = [...rows]
    .sort((a, b) => a.verdict.priority - b.verdict.priority || b.modeReliability - a.modeReliability)
    .slice(0, TOP_BANDS);
  const windowsFor = (band: string) => {
    const index = prediction.bands.findIndex((b) => b.name === band);
    const byClockHour = new Array<number>(24).fill(0);
    for (const h of hours) byClockHour[clockHour(h.utcHour)] = h.frequencies[index].reliability;
    return openWindows(byClockHour);
  };
  const wwv = conditions?.products.find((p) => p.kind === "wwv");
  const alert = wwv?.stored?.product.kind === "wwv" ? wwv.stored.product : null;

  return (
    <section className="field">
      {error && <p className="error">{error}</p>}
      <p className="field-path">
        {prediction.txLocator} → {prediction.rxLocator} · {prediction.distanceKm.toFixed(0)} km ·{" "}
        {longPath ? "long path" : "short path"} · {modeLabel} · {hourLabel(hour.utcHour)} UTC
      </p>

      <div className="field-bands">
        {ranked.map((row, i) => {
          const windows = windowsFor(row.band);
          return (
            <article key={row.band} className="card">
              <p className="field-rank">{i === 0 ? "Try first" : "Then"}</p>
              <p className="field-band">{row.band}</p>
              <p className="field-verdict">
                <span className={`verdict verdict-${row.verdict.priority}`} aria-hidden="true">
                  {verdictIcon(row.verdict.priority)}
                </span>{" "}
                {row.verdict.label}
              </p>
              <p>{row.verdict.detail}</p>
              {modeCaution(row, modeLabel, prediction.requiredSnrDbHz) && (
                <p className="caution">{modeCaution(row, modeLabel, prediction.requiredSnrDbHz)}</p>
              )}
              <dl>
                <dt>Predicted, {modeLabel}</dt>
                <dd>{(row.modeReliability * 100).toFixed(0)}% of days</dd>
                <dt>Heard that way</dt>
                <dd>
                  {OBSERVED[row.observed]}
                  {row.evidenceStations > 0 && `, ${row.evidenceStations} stations`}
                </dd>
                <dt>Good hours (UTC)</dt>
                <dd>{windows.length > 0 ? windows.join(", ") : "none today"}</dd>
              </dl>
            </article>
          );
        })}
      </div>
      {rows.length > 0 && ranked.every((row) => row.verdict.priority >= 6) && (
        <p className="banner">
          No band looks dependable for this path at this hour. Check the good hours above, or try
          another hour with the selector.
        </p>
      )}

      <div className="field-status">
        <article className="card">
          <h3>Conditions</h3>
          <p>{conditions?.storm ?? "No geomagnetic storm in the current data."}</p>
          {alert && wwv?.ageSeconds != null ? (
            <p>
              Solar flux {alert.solarFlux ?? "?"}, A index {alert.aIndex ?? "?"}, K index{" "}
              {alert.kIndex ?? "?"}.{" "}
              <span className={wwv.stale ? "error" : "hint"}>
                {age(wwv.ageSeconds)} old{wwv.stale && ", stale"}.
              </span>
            </p>
          ) : (
            <p className="hint">No solar indices received yet. See the Conditions tab.</p>
          )}
        </article>
        <article className="card">
          <h3>Data age</h3>
          <dl>
            <dt>Prediction</dt>
            <dd>
              Climatological, sunspot number {prediction.ssn.value}
              {conditions &&
                `, table ${age(conditions.ssnTable.ageSeconds)} old${conditions.ssnTable.stale ? " (stale)" : ""}`}
            </dd>
            <dt>Last decode</dt>
            <dd>
              {last
                ? `${age(now - last.timeUtc)} ago on ${last.band}`
                : "none stored; see the Heard tab"}
            </dd>
            <dt>Evidence span</dt>
            <dd>stations heard in the last {EVIDENCE_MINUTES} minutes</dd>
          </dl>
        </article>
      </div>

      <WorldMap
        from={prediction.tx}
        to={prediction.rx}
        longPath={longPath}
        month={month}
        clockHour={clockHour(hour.utcHour)}
        coverage={null}
        heard={[]}
        picking={false}
        onPick={() => {}}
      />
      <p className="note">
        Everything on this screen comes from data on this computer. The recommendation compares the
        FT8 prediction with FT8 stations heard toward the destination; see the Compare tab for the
        full table and what it does and does not show.
      </p>
    </section>
  );
}
