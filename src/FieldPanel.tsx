import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { modeCaution, useComparison, verdictIcon } from "./comparison";
import { WorldMap } from "./WorldMap";
import { WindowsText } from "./BestBands";
import { Conditions, Observation, PathDetail } from "./types";
import { byClockHour, clockHour, describeWindows } from "./tiers";
import { hourBoth, Zone } from "./localtime";
import { age, Evidence, HourStrip, OBSERVED_WORDS, pct, Ring, signedDb } from "./ui";

const EVIDENCE_MINUTES = 60;
const TOP_BANDS = 3;
const POLL_MS = 5000;

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
  zone: Zone;
  conditions: Conditions | null;
  nowClock: number;
};

/** One screen for working a path: what to try, when, and how fresh the data is. */
export function FieldPanel({ detail, hourIndex, modeLabel, longPath, month, zone, conditions, nowClock }: Props) {
  const [rows, error] = useComparison(detail, hourIndex, EVIDENCE_MINUTES);
  const last = useLastDecode();
  const prediction = detail.prediction;
  const hours = prediction.run.prediction.hours;
  const hour = hours[hourIndex];
  const clock = clockHour(hour.utcHour);
  const now = Date.now() / 1000;

  const ranked = [...rows]
    .sort((a, b) => a.verdict.priority - b.verdict.priority || b.modeReliability - a.modeReliability)
    .slice(0, TOP_BANDS);
  const daily = (band: string) => byClockHour(hours, prediction.bands.findIndex((b) => b.name === band));
  const wwv = conditions?.products.find((p) => p.kind === "wwv");
  const sun = wwv?.stored?.product.kind === "wwv" ? wwv.stored.product : null;
  const nothingDependable = rows.length > 0 && ranked.every((row) => row.verdict.priority >= 6);

  return (
    <section>
      <div className="view-head">
        <h2>
          {prediction.txLocator} → {prediction.rxLocator}
        </h2>
        <span className="hint">
          {prediction.distanceKm.toFixed(0)} km {longPath ? "long" : "short"} path, {modeLabel},{" "}
          {hourBoth(clock, zone)}
        </span>
      </div>
      {error && <p className="error">{error}</p>}

      <div className="field">
        <div className="field-bands">
          {nothingDependable && (
            <p className="lead">
              No band looks dependable at this hour. The strips show when each one opens; step the hour
              to see a better time.
            </p>
          )}
          {ranked.map((row, i) => {
            const values = daily(row.band);
            const caution = modeCaution(row, modeLabel, prediction.requiredSnrDbHz);
            return (
              <article key={row.band} className={`field-band${i === 0 ? " first" : ""}`}>
                <div className="big">
                  {row.band}
                  <small>{i === 0 ? "Try first" : "Then"}</small>
                </div>
                <div className="verdict-line">
                  <span className={`verdict verdict-${row.verdict.priority}`} aria-hidden="true">
                    {verdictIcon(row.verdict.priority)}
                  </span>{" "}
                  {row.verdict.label}
                  <span className="hint" style={{ fontWeight: 400, fontSize: 13 }}>
                    {" "}
                    {row.verdict.detail}
                  </span>
                </div>
                <div className="facts">
                  <span>
                    Model{" "}
                    {modeLabel.split(" ")[0] === "FT8" ? (
                      <>
                        <b>{pct(row.ft8Reliability)}</b> FT8
                      </>
                    ) : (
                      <>
                        <b>{pct(row.modeReliability)}</b> {modeLabel.split(" ")[0]}, <b>{pct(row.ft8Reliability)}</b> FT8
                      </>
                    )}
                  </span>
                  <span>
                    <Evidence tier={row.observed} /> {OBSERVED_WORDS[row.observed]}
                    {row.evidenceStations > 0 && `, ${row.evidenceStations}`}
                  </span>
                  {row.hearingStations > 0 && (
                    <span>
                      <Ring /> {row.hearingYou > 0 ? `${row.hearingYou} heard you` : `${row.hearingStations} hear your area`}
                      {row.hearingBestReportDb !== null && `, best ${signedDb(row.hearingBestReportDb)}`}
                    </span>
                  )}
                </div>
                <div className="facts">
                  <HourStrip byClockHour={values} selected={clock} now={nowClock} label={`${row.band} through the day`} />
                  <span>
                    Good hours <WindowsText windows={describeWindows(values, zone)} none="none today" />
                  </span>
                </div>
                {caution && <div className="caution" style={{ gridColumn: 2 }}>{caution}</div>}
              </article>
            );
          })}
        </div>

        <div className="field-side">
          <WorldMap
            from={prediction.tx}
            to={prediction.rx}
            longPath={longPath}
            month={month}
            clockHour={clock}
            coverage={null}
            heard={[]}
            picking={false}
            onPick={() => {}}
          />
          <div className="panel">
            <dl className="readings">
              <dt>Conditions</dt>
              <dd>
                {sun ? (
                  <>
                    Solar flux <span className="num">{sun.solarFlux ?? "?"}</span>, A{" "}
                    <span className="num">{sun.aIndex ?? "?"}</span>, K{" "}
                    <span className="num">{sun.kIndex ?? "?"}</span>
                    {conditions?.storm && <span className="error">, geomagnetic storm</span>}
                  </>
                ) : conditions?.storm ? (
                  <span className="error">Geomagnetic storm</span>
                ) : (
                  <span className="hint">No solar data yet</span>
                )}
              </dd>
              <dt>Solar data</dt>
              <dd className={wwv?.stale ? "stale" : undefined}>
                {wwv?.ageSeconds != null ? `${age(wwv.ageSeconds)} old${wwv.stale ? ", stale" : ""}` : "not received"}
              </dd>
              <dt>Prediction</dt>
              <dd>
                Monthly model, sunspot number <span className="num">{prediction.ssn.value}</span>
                {conditions && (
                  <span className={conditions.ssnTable.stale ? "stale" : "hint"}>
                    , table {age(conditions.ssnTable.ageSeconds)} old
                  </span>
                )}
              </dd>
              <dt>Last decode</dt>
              <dd>{last ? `${age(now - last.timeUtc)} ago on ${last.band}` : <span className="hint">none stored</span>}</dd>
              <dt>Heard span</dt>
              <dd>last {EVIDENCE_MINUTES} minutes</dd>
            </dl>
          </div>
        </div>
      </div>
      <p className="note">
        Everything here comes from data on this computer. The recommendation sets the FT8 prediction
        beside FT8 stations heard toward the destination; the Compare view has every band.
      </p>
    </section>
  );
}
