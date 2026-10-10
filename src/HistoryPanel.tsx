import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { CalibrationBin, CalibrationReport } from "./types";

type Props = {
  /** The From field, used only when no stored decode carries the receiver's locator. */
  rxPosition: string;
  /** The operator's own noise level, applied at the receiving end. */
  noiseDb: number | null;
};

type Progress = { done: number; total: number };

const W = 900;
const H = 330;
const MARGIN = { top: 28, right: 16, bottom: 70, left: 52 };
const PLOT_W = W - MARGIN.left - MARGIN.right;
const PLOT_H = H - MARGIN.top - MARGIN.bottom;
const PERCENT_TICKS = [0, 25, 50, 75, 100];
const RESIDENTIAL_NOISE_DB = 145;
/** Bins with fewer listening hours than this are drawn faint: too few to read much into. */
const THIN = 20;

const binLabel = (i: number, bins: number) => `${(i * 100) / bins}–${((i + 1) * 100) / bins}`;
const rateOf = (bin: CalibrationBin) => (bin.opportunities === 0 ? null : bin.heard / bin.opportunities);
const percent = (share: number) => `${(share * 100).toFixed(0)}%`;

/** 95% Wilson score interval for a proportion. */
function wilson(heard: number, n: number): [number, number] {
  if (n === 0) return [0, 0];
  const z = 1.96;
  const p = heard / n;
  const centre = (p + (z * z) / (2 * n)) / (1 + (z * z) / n);
  const half = (z * Math.sqrt((p * (1 - p)) / n + (z * z) / (4 * n * n))) / (1 + (z * z) / n);
  return [Math.max(0, centre - half), Math.min(1, centre + half)];
}

/** A column from the baseline up to `top`, its data end rounded. */
function column(x: number, top: number, width: number, base: number): string {
  const r = Math.min(4, (base - top) / 2, width / 2);
  return (
    `M${x},${base} L${x},${top + r} Q${x},${top} ${x + r},${top} ` +
    `L${x + width - r},${top} Q${x + width},${top} ${x + width},${top + r} L${x + width},${base} Z`
  );
}

/** Bands with data, 80 m first. */
function bandOrder(report: CalibrationReport): string[] {
  return Object.keys(report.byBand).sort((a, b) => parseInt(b, 10) - parseInt(a, 10));
}

/** How often predicted reliability was borne out by what this station heard. */
export function HistoryPanel({ rxPosition, noiseDb }: Props) {
  const [report, setReport] = useState<CalibrationReport | null>(null);
  const [progress, setProgress] = useState<Progress | null>(null);
  const [error, setError] = useState("");
  const [band, setBand] = useState("all");
  const [hover, setHover] = useState<number | null>(null);

  useEffect(() => {
    let active = true;
    const unlisten = listen<Progress>("calibration-progress", (event) => {
      if (active) setProgress(event.payload);
    });
    return () => {
      active = false;
      unlisten.then((stop) => stop()).catch(() => {});
    };
  }, []);

  const run = async () => {
    setError("");
    setProgress({ done: 0, total: 0 });
    try {
      const next = await invoke<CalibrationReport>("calibration_report", {
        query: { rxPosition, noiseDb: noiseDb ?? RESIDENTIAL_NOISE_DB },
      });
      setReport(next);
      if (!(band in next.byBand)) setBand("all");
    } catch (e) {
      setError(String(e));
    } finally {
      setProgress(null);
    }
  };

  const bins = report === null ? null : band === "all" ? report.overall : report.byBand[band];
  const slot = PLOT_W / (bins?.length ?? 1);
  const barWidth = Math.min(24, slot * 0.5);
  const x = (i: number) => MARGIN.left + i * slot + (slot - barWidth) / 2;
  const y = (rate: number) => MARGIN.top + PLOT_H - rate * PLOT_H;
  const shown = hover !== null && bins ? bins[hover] : null;
  const decodes = bins?.reduce((n, b) => n + b.decodes, 0) ?? 0;
  const first = bins ? rateOf(bins[0]) : null;
  const last = bins ? rateOf(bins[bins.length - 1]) : null;

  return (
    <section className="history">
      <div className="view-head">
        <h2>Does the model match what you hear?</h2>
      </div>
      <div className="controls">
        <button type="button" className={report ? undefined : "primary"} onClick={run} disabled={progress !== null}>
          {progress === null ? (report ? "Check again" : "Check the model against my decodes") : "Predicting…"}
        </button>
        {progress !== null && progress.total > 0 && (
          <span className="hint">
            {progress.done} of {progress.total} prediction runs
          </span>
        )}
        {report && (
          <label className="inline">
            Band
            <select value={band} onChange={(e) => setBand(e.target.value)}>
              <option value="all">All bands</option>
              {bandOrder(report).map((b) => (
                <option key={b} value={b}>
                  {b}
                </option>
              ))}
            </select>
          </label>
        )}
      </div>
      {error && <p className="error">{error}</p>}

      {report === null && progress === null && (
        <p className="note">
          This takes every decode you have stored, asks the model what it would have predicted for that
          place at that hour and month, and compares. The first run over a long log takes a few
          minutes; later runs reuse its predictions.
        </p>
      )}

      {report && bins && report.decodesUsed === 0 && (
        <p className="note">
          Nothing to check yet: no stored decode has a locator. Turn on listening in Heard, or add a
          WSJT-X log there.
        </p>
      )}

      {report && bins && report.decodesUsed > 0 && (
        <>
          <p className="summary">
            {decodes.toLocaleString()} decodes checked, receiver {report.receiver}.{" "}
            {first !== null && last !== null && (
              <>
                Where the model said under 10%, those places were heard in {percent(first)} of the hours
                you listened; where it said 90% or more, in {percent(last)}.
              </>
            )}
          </p>
          <div className="legend">
            <span>
              <span className="swatch meas" /> How often heard, with its 95% range
            </span>
            <span className="hint">Faint columns rest on fewer than {THIN} listening hours.</span>
          </div>
          <div className="chart" style={{ maxWidth: W }}>
            <svg
              viewBox={`0 0 ${W} ${H}`}
              role="img"
              aria-label="Columns of how often places were heard, grouped by what the model predicted for them"
            >
              {PERCENT_TICKS.map((p) => (
                <g key={p}>
                  <line className="grid" x1={MARGIN.left} x2={MARGIN.left + PLOT_W} y1={y(p / 100)} y2={y(p / 100)} />
                  <text className="tick" x={MARGIN.left - 8} y={y(p / 100) + 4} textAnchor="end">
                    {p}%
                  </text>
                </g>
              ))}
              <text className="axis-title" x={MARGIN.left - 8} y={MARGIN.top - 12} textAnchor="end">
                Heard
              </text>
              <line className="axis" x1={MARGIN.left} x2={MARGIN.left + PLOT_W} y1={y(0)} y2={y(0)} />
              {bins.map((b, i) => {
                const rate = rateOf(b);
                const [lo, hi] = wilson(b.heard, b.opportunities);
                const dim = (hover !== null && hover !== i) || b.opportunities < THIN;
                const cx = x(i) + barWidth / 2;
                return (
                  <g key={i} onMouseEnter={() => setHover(i)} onMouseLeave={() => setHover(null)}>
                    <rect className="bar-hit" x={MARGIN.left + i * slot} y={MARGIN.top} width={slot} height={PLOT_H + 40} />
                    {rate !== null && <path className={`bar${dim ? " dim" : ""}`} d={column(x(i), y(rate), barWidth, y(0))} />}
                    {rate !== null && b.opportunities > 1 && (
                      <g className="whisker">
                        <line x1={cx} x2={cx} y1={y(hi)} y2={y(lo)} />
                        <line x1={cx - 5} x2={cx + 5} y1={y(hi)} y2={y(hi)} />
                        <line x1={cx - 5} x2={cx + 5} y1={y(lo)} y2={y(lo)} />
                      </g>
                    )}
                    <text className="tick" x={cx} y={H - MARGIN.bottom + 18} textAnchor="middle">
                      {binLabel(i, bins.length)}
                    </text>
                    <text className="tick" x={cx} y={H - MARGIN.bottom + 34} textAnchor="middle" style={{ opacity: 0.7 }}>
                      {b.opportunities.toLocaleString()}
                    </text>
                  </g>
                );
              })}
              <text className="axis-title" x={MARGIN.left - 8} y={H - MARGIN.bottom + 34} textAnchor="end">
                hours
              </text>
              <text className="axis-title" x={MARGIN.left + PLOT_W / 2} y={H - 12} textAnchor="middle">
                What the model predicted for the path, % of days
              </text>
            </svg>
            {shown && hover !== null && (
              <div
                className="tooltip"
                style={{
                  left: `${((x(hover) + barWidth) / W) * 100}%`,
                  transform: hover > 5 ? `translateX(calc(-100% - ${barWidth + 12}px))` : "translateX(10px)",
                }}
              >
                <div className="tooltip-title">Model said {binLabel(hover, bins.length)}%</div>
                <div>
                  Heard in <strong>{shown.heard.toLocaleString()}</strong> of {shown.opportunities.toLocaleString()}{" "}
                  hours listened
                  {rateOf(shown) !== null && `, ${percent(rateOf(shown)!)}`}
                </div>
                {shown.opportunities > 1 && (
                  <div className="hint">
                    95% range {percent(wilson(shown.heard, shown.opportunities)[0])} to{" "}
                    {percent(wilson(shown.heard, shown.opportunities)[1])}
                  </div>
                )}
                <div>
                  <strong>{shown.decodes.toLocaleString()}</strong> decodes
                </div>
              </div>
            )}
          </div>
          <p className="note">
            How to read it: the places you heard are grouped by what the model predicted for them, from
            almost never on the left to almost always on the right. Each column is how often you actually
            heard those places in the hours you were listening; the whisker is its 95% range, and the
            figure under it is how many listening hours it rests on. If the model is any good the columns
            rise from left to right. They stay well below the prediction because a station is only heard
            when someone there is transmitting, so compare their shape, not their height. The other
            station is assumed to run 100 W into a simple antenna; your own noise level is used at this
            end.
          </p>
          <details>
            <summary>Show as a table</summary>
            <table className="results compact">
              <thead>
                <tr>
                  <th>Model said</th>
                  <th>Decodes</th>
                  <th>Hours listened</th>
                  <th>Hours heard</th>
                  <th>How often</th>
                  <th>95% range</th>
                </tr>
              </thead>
              <tbody>
                {bins.map((b, i) => {
                  const rate = rateOf(b);
                  const [lo, hi] = wilson(b.heard, b.opportunities);
                  return (
                    <tr key={i}>
                      <th>{binLabel(i, bins.length)}%</th>
                      <td>{b.decodes.toLocaleString()}</td>
                      <td>{b.opportunities.toLocaleString()}</td>
                      <td>{b.heard.toLocaleString()}</td>
                      <td>{rate === null ? "—" : percent(rate)}</td>
                      <td>{b.opportunities > 1 ? `${percent(lo)}–${percent(hi)}` : "—"}</td>
                    </tr>
                  );
                })}
              </tbody>
            </table>
          </details>
          <p className="hint">
            {report.decodesSkipped.toLocaleString()} decodes left out: no locator, a band the model does not
            cover, or a month outside the sunspot table. {report.circuits.toLocaleString()} paths and hours
            predicted.
          </p>
        </>
      )}
    </section>
  );
}
