import { useState } from "react";
import { modeCaution, OBSERVED, useComparison, verdictIcon } from "./comparison";
import { BandComparison, PathDetail } from "./types";
import { hourLabel, shade } from "./tiers";

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
    label: "Combined",
    order: (a, b) => a.verdict.priority - b.verdict.priority || b.modeReliability - a.modeReliability,
  },
  { key: "predicted", label: "Predicted only", order: (a, b) => b.modeReliability - a.modeReliability },
  {
    key: "observed",
    label: "Observed only",
    order: (a, b) =>
      OBSERVED_RANK[a.observed] - OBSERVED_RANK[b.observed] || b.evidenceStations - a.evidenceStations,
  },
];

const percent = (share: number) => `${(share * 100).toFixed(0)}%`;

type Props = { detail: PathDetail; hourIndex: number; modeLabel: string };

/** Prediction for the path beside what has been heard toward the destination. */
export function ComparePanel({ detail, hourIndex, modeLabel }: Props) {
  const [minutes, setMinutes] = useState(60);
  const [sortKey, setSortKey] = useState("combined");
  const [rows, error] = useComparison(detail, hourIndex, minutes);
  const sort = SORTS.find((s) => s.key === sortKey) ?? SORTS[0];
  const hour = detail.prediction.run.prediction.hours[hourIndex];

  return (
    <section>
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
        <label className="inline">
          Rank by
          <select value={sortKey} onChange={(e) => setSortKey(e.target.value)}>
            {SORTS.map((s) => (
              <option key={s.key} value={s.key}>
                {s.label}
              </option>
            ))}
          </select>
        </label>
      </div>
      {error && <p className="error">{error}</p>}

      <h3>Predicted and observed at {hourLabel(hour.utcHour)} UTC</h3>
      <div className="scroll-x">
        <table className="results compact compare">
          <thead>
            <tr>
              <th rowSpan={2}>Band</th>
              <th rowSpan={2}>Recommendation</th>
              <th colSpan={2}>Predicted reliability</th>
              <th colSpan={3}>Heard toward the destination</th>
              <th colSpan={2}>Heard on the band</th>
            </tr>
            <tr>
              <th>{modeLabel}</th>
              <th>FT8</th>
              <th>Evidence</th>
              <th>Stations</th>
              <th>Best SNR</th>
              <th>Callsigns</th>
              <th>Listened</th>
            </tr>
          </thead>
          <tbody>
            {[...rows].sort(sort.order).map((row) => (
              <tr key={row.band}>
                <th>{row.band}</th>
                <td className="left">
                  <span className={`verdict verdict-${row.verdict.priority}`} aria-hidden="true">
                    {verdictIcon(row.verdict.priority)}
                  </span>{" "}
                  <strong>{row.verdict.label}</strong>
                  <div className="hint">{row.verdict.detail}</div>
                  {modeCaution(row, modeLabel, detail.prediction.requiredSnrDbHz) && (
                    <div className="caution">{modeCaution(row, modeLabel, detail.prediction.requiredSnrDbHz)}</div>
                  )}
                </td>
                <td style={{ background: shade(row.modeReliability) }}>{percent(row.modeReliability)}</td>
                <td style={{ background: shade(row.ft8Reliability) }}>{percent(row.ft8Reliability)}</td>
                <td className="left">{OBSERVED[row.observed]}</td>
                <td className="left">
                  {row.evidenceStations}
                  {row.evidenceExamples.length > 0 && (
                    <span className="hint"> ({row.evidenceExamples.join(", ")})</span>
                  )}
                </td>
                <td>{row.evidenceBestSnrDb === null ? "—" : `${row.evidenceBestSnrDb} dB`}</td>
                <td>{row.bandCallsigns}</td>
                <td>{row.periods === 0 ? "—" : `${row.periods.toFixed(0)} periods`}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
      <p className="note">
        The recommendation compares the FT8 prediction for this path with FT8 stations heard toward
        the destination: within 1,500 km of it, or within 15° of its bearing and at least 60% as
        far. Under four transmit periods of listening on a band counts as not listened to. Three
        stations is moderate evidence and eight is strong.
      </p>
      <p className="note">
        Hearing stations that way shows the band is open in that direction for FT8. It does not
        show that they can hear you, and FT8 gets through on about 25 dB less signal than SSB, so
        read the {modeLabel} column for your own mode. Nothing heard is not proof that a band is
        closed: it depends on who is transmitting.
      </p>
    </section>
  );
}
