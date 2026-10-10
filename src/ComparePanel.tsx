import { useState } from "react";
import { modeCaution, useComparison, verdictIcon } from "./comparison";
import { BandComparison, PathDetail } from "./types";
import { clockHour } from "./tiers";
import { hourBoth, Zone } from "./localtime";
import { Evidence, Meter, OBSERVED_WORDS, Ring, signedDb } from "./ui";

const WINDOWS = [
  { minutes: 15, label: "15 minutes" },
  { minutes: 60, label: "hour" },
  { minutes: 360, label: "6 hours" },
];

const OBSERVED_RANK: Record<BandComparison["observed"], number> = {
  strong: 0,
  moderate: 1,
  limited: 2,
  none: 3,
  notSampled: 4,
};

const SORTS: { key: string; label: string; order: (a: BandComparison, b: BandComparison) => number }[] = [
  {
    key: "combined",
    label: "Recommendation",
    order: (a, b) => a.verdict.priority - b.verdict.priority || b.modeReliability - a.modeReliability,
  },
  { key: "predicted", label: "Model", order: (a, b) => b.ft8Reliability - a.ft8Reliability },
  {
    key: "observed",
    label: "Heard",
    order: (a, b) =>
      OBSERVED_RANK[a.observed] - OBSERVED_RANK[b.observed] || b.evidenceStations - a.evidenceStations,
  },
];

type Props = { detail: PathDetail; hourIndex: number; modeLabel: string; zone: Zone };

/** The model's prediction for the path beside what was heard toward the destination. */
export function ComparePanel({ detail, hourIndex, modeLabel, zone }: Props) {
  const [minutes, setMinutes] = useState(60);
  const [sortKey, setSortKey] = useState("combined");
  const [rows, error] = useComparison(detail, hourIndex, minutes);
  const sort = SORTS.find((s) => s.key === sortKey) ?? SORTS[0];
  const hour = detail.prediction.run.prediction.hours[hourIndex];
  const listened = rows.filter((r) => r.observed !== "notSampled").length;

  return (
    <section>
      <div className="view-head">
        <h2>Model against what you heard</h2>
        <span className="hint">
          Prediction for {hourBoth(clockHour(hour.utcHour), zone)}, toward {detail.prediction.rxLocator}
        </span>
      </div>

      <div className="controls">
        <label className="inline">
          Heard in the last
          <select value={minutes} onChange={(e) => setMinutes(Number(e.target.value))}>
            {WINDOWS.map((w) => (
              <option key={w.minutes} value={w.minutes}>
                {w.label}
              </option>
            ))}
          </select>
        </label>
        <div className="segmented" role="group" aria-label="Order the bands by">
          {SORTS.map((s) => (
            <button key={s.key} type="button" aria-pressed={s.key === sortKey} onClick={() => setSortKey(s.key)}>
              {s.label}
            </button>
          ))}
        </div>
        <span className="hint">
          {listened} of {rows.length} bands listened to long enough to tell
        </span>
      </div>
      {error && <p className="error">{error}</p>}

      <div className="legend">
        <span>
          <span className="swatch model" /> Model: share of days VOACAP predicts the path works
        </span>
        <span>
          <span className="swatch meas" /> Heard: FT8 stations your receiver decoded toward the destination
        </span>
        <span>
          <span className="swatch hatch" /> Not listened to long enough to tell
        </span>
        <span>
          <Ring /> Hears your area: stations that way heard reporting you or a station near you
        </span>
      </div>

      <div className="scroll-x">
        <table className="data compare wide">
          <thead>
            <tr>
              <th className="left">Band</th>
              <th className="left">Recommendation</th>
              <th>Model</th>
              <th className="left">Heard toward the destination</th>
              <th className="left">Hears your area</th>
              <th className="whole-band">On the whole band</th>
            </tr>
          </thead>
          <tbody>
            {[...rows].sort(sort.order).map((row) => {
              const caution = modeCaution(row, modeLabel, detail.prediction.requiredSnrDbHz);
              return (
                <tr key={row.band}>
                  <th className="band">{row.band}</th>
                  <td className="left rec">
                    <span className={`verdict verdict-${row.verdict.priority}`} aria-hidden="true">
                      {verdictIcon(row.verdict.priority)}
                    </span>{" "}
                    <strong>{row.verdict.label}</strong>
                    <span className="hint">{row.verdict.detail}</span>
                    {caution && <span className="caution">{caution}</span>}
                  </td>
                  <td>
                    <div className="pair">
                      <Meter value={row.ft8Reliability} label="FT8" />
                      {modeLabel.split(" ")[0] !== "FT8" && <Meter value={row.modeReliability} label={modeLabel.split(" ")[0]} />}
                    </div>
                  </td>
                  <td>
                    <div className="heardcell">
                      <span>
                        <Evidence tier={row.observed} /> {OBSERVED_WORDS[row.observed]}
                        {row.evidenceStations > 0 &&
                          `, ${row.evidenceStations} ${row.evidenceStations === 1 ? "station" : "stations"}`}
                      </span>
                      {row.evidenceExamples.length > 0 && (
                        <span className="examples">
                          {row.evidenceExamples.join("  ")}
                          {row.evidenceBestSnrDb !== null && `  best ${row.evidenceBestSnrDb} dB`}
                        </span>
                      )}
                    </div>
                  </td>
                  <td>
                    <div className="heardcell">
                      {row.hearingStations > 0 ? (
                        <>
                          <span>
                            <Ring /> {row.hearingStations} {row.hearingStations === 1 ? "station" : "stations"}
                            {row.hearingYou > 0 && `, ${row.hearingYou} heard you`}
                          </span>
                          <span className="examples">
                            {row.hearingExamples.join("  ")}
                            {row.hearingBestReportDb !== null && `  best ${signedDb(row.hearingBestReportDb)}`}
                          </span>
                        </>
                      ) : (
                        <span className="hint">none heard</span>
                      )}
                    </div>
                  </td>
                  <td className="whole-band">
                    {row.periods === 0 ? (
                      <span className="hint">not listened to</span>
                    ) : (
                      <>
                        <span className="num">{row.bandCallsigns}</span> {row.bandCallsigns === 1 ? "callsign" : "callsigns"}
                        <div className="hint">
                          over <span className="num">{row.periods.toFixed(0)}</span> periods
                        </div>
                      </>
                    )}
                  </td>
                </tr>
              );
            })}
          </tbody>
        </table>
      </div>

      <p className="note">
        The recommendation sets the FT8 prediction beside FT8 stations heard toward the destination:
        within 1,500 km of it, or within 15° of its bearing and at least 60% as far. Under four
        transmit periods of listening counts as not listened to. Three stations is moderate evidence,
        eight is strong.
      </p>
      <p className="note">
        "Hears your area" is the other direction. It counts stations toward the destination that this
        receiver heard sending a signal report to you, or to a station near you: within 300 km, or for a
        distant sender up to 15% of its distance and never more than 1,000 km. Their report is how well
        they hear your part of the world. It sits beside the recommendation and does not change it.
      </p>
      <p className="note">
        Hearing stations that way shows the band is open that way for FT8. It does not show they can
        hear you, and FT8 gets through on about 25 dB less signal than SSB, so read the {modeLabel}{" "}
        bar for your own mode. Nothing heard is not proof a band is closed: it depends on who is
        transmitting.
      </p>
    </section>
  );
}
