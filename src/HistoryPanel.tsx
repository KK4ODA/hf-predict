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

const WIDTH = 760;
const HEIGHT = 300;
const MARGIN = { top: 26, right: 16, bottom: 50, left: 48 };
const PLOT_W = WIDTH - MARGIN.left - MARGIN.right;
const PLOT_H = HEIGHT - MARGIN.top - MARGIN.bottom;
const PERCENT_TICKS = [0, 25, 50, 75, 100];
const RESIDENTIAL_NOISE_DB = 145;

const binLabel = (i: number, bins: number) => `${(i * 100) / bins}–${((i + 1) * 100) / bins}`;
const rateOf = (bin: CalibrationBin) => (bin.opportunities === 0 ? null : bin.heard / bin.opportunities);
const percent = (share: number) => `${(share * 100).toFixed(0)}%`;

/** A bar from the baseline up to `top`, with its data end rounded. */
function bar(x: number, top: number, width: number, base: number): string {
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

function Summary({ report, bins }: { report: CalibrationReport; bins: CalibrationBin[] }) {
  const decodes = bins.reduce((n, b) => n + b.decodes, 0);
  const goodShare = bins.slice(7).reduce((n, b) => n + b.decodes, 0) / Math.max(1, decodes);
  const first = rateOf(bins[0]);
  const last = rateOf(bins[bins.length - 1]);
  return (
    <p>
      {decodes.toLocaleString()} decodes with a locator, {percent(goodShare)} of them on paths
      predicted at 70% or better.{" "}
      {first !== null && last !== null && (
        <>
          A locator heard that month was heard in {percent(first)} of listening hours when the
          prediction was under 10%, and in {percent(last)} when it was 90% or more.
        </>
      )}{" "}
      <span className="hint">
        Receiver {report.receiver}; {report.circuits.toLocaleString()} paths and hours predicted.
      </span>
    </p>
  );
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
  const barWidth = slot * 0.6;
  const x = (i: number) => MARGIN.left + i * slot + (slot - barWidth) / 2;
  const y = (rate: number) => MARGIN.top + PLOT_H - rate * PLOT_H;
  const shown = hover !== null && bins ? bins[hover] : null;

  return (
    <section>
      <div className="controls">
        <button type="button" onClick={run} disabled={progress !== null}>
          {progress === null ? (report ? "Run again" : "Check predictions against decodes") : "Predicting…"}
        </button>
        {progress !== null && progress.total > 0 && (
          <span className="hint">
            {progress.done} of {progress.total} engine runs
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
          Takes every stored decode with a locator, predicts that path for its hour and month, and
          shows whether stations were heard more often where the model said they would be. The
          first run over a long log takes a few minutes; later runs reuse its predictions.
        </p>
      )}

      {report && bins && report.decodesUsed === 0 && (
        <p className="note">
          No stored decode carries a locator yet. Enable the listener on the Heard tab, or import
          a WSJT-X ALL.TXT there.
        </p>
      )}

      {report && bins && report.decodesUsed > 0 && (
        <>
          <h3>Heard against predicted, {band === "all" ? "all bands" : band}</h3>
          <Summary report={report} bins={bins} />
          <div className="chart">
            <svg
              viewBox={`0 0 ${WIDTH} ${HEIGHT}`}
              role="img"
              aria-label="Bar chart of the share of listening hours in which a heard locator was heard, by predicted reliability"
            >
              {PERCENT_TICKS.map((p) => (
                <g key={p}>
                  <line
                    className="grid"
                    x1={MARGIN.left}
                    x2={MARGIN.left + PLOT_W}
                    y1={y(p / 100)}
                    y2={y(p / 100)}
                  />
                  <text className="tick" x={MARGIN.left - 6} y={y(p / 100) + 3.5} textAnchor="end">
                    {p}%
                  </text>
                </g>
              ))}
              <text className="tick" x={MARGIN.left - 6} y={MARGIN.top - 10} textAnchor="end">
                heard
              </text>
              {bins.map((b, i) => {
                const rate = rateOf(b);
                const dim = hover !== null && hover !== i;
                return (
                  <g key={i} onMouseEnter={() => setHover(i)} onMouseLeave={() => setHover(null)}>
                    <rect
                      className="bar-hit"
                      x={MARGIN.left + i * slot}
                      y={MARGIN.top}
                      width={slot}
                      height={PLOT_H}
                    />
                    {rate !== null && (
                      <path className={`bar${dim ? " dim" : ""}`} d={bar(x(i), y(rate), barWidth, y(0))} />
                    )}
                    {rate !== null && (i === 0 || i === bins.length - 1) && (
                      <text className="value" x={x(i) + barWidth / 2} y={y(rate) - 6} textAnchor="middle">
                        {percent(rate)}
                      </text>
                    )}
                    <text className="tick" x={x(i) + barWidth / 2} y={HEIGHT - 30} textAnchor="middle">
                      {binLabel(i, bins.length)}
                    </text>
                  </g>
                );
              })}
              <text className="tick" x={MARGIN.left + PLOT_W / 2} y={HEIGHT - 10} textAnchor="middle">
                predicted reliability, % of days
              </text>
            </svg>
            {shown && hover !== null && (
              <div
                className="tooltip"
                style={{
                  left: `${((x(hover) + barWidth / 2) / WIDTH) * 100}%`,
                  transform: hover > 5 ? "translateX(calc(-100% - 12px))" : "translateX(12px)",
                }}
              >
                <div className="tooltip-title">Predicted {binLabel(hover, bins.length)}%</div>
                <div>
                  <strong>{shown.decodes.toLocaleString()}</strong> decodes
                </div>
                <div>
                  heard in <strong>{shown.heard.toLocaleString()}</strong> of{" "}
                  {shown.opportunities.toLocaleString()} listening hours
                  {rateOf(shown) !== null && ` (${percent(rateOf(shown)!)})`}
                </div>
              </div>
            )}
          </div>
          <p className="note">
            Each bar: of the hours this station was listening on the band, the share in which a
            locator heard at some point that month was heard, grouped by what the model predicted
            for that path at that hour. A station is heard only when it is also transmitting, so
            the bars sit well below the predicted reliability; what matters is that they climb
            from left to right. Stations are assumed to run 100 W into an isotropic antenna, with
            your noise level at the receiving end. Hours with no decode at all from an imported
            log are not counted as listening, which flattens the curve a little.
          </p>
          <details>
            <summary>Show as table</summary>
            <table className="results compact">
              <thead>
                <tr>
                  <th>Predicted</th>
                  <th>Decodes</th>
                  <th>Share</th>
                  <th>Listening hours</th>
                  <th>Heard</th>
                  <th>Rate</th>
                </tr>
              </thead>
              <tbody>
                {bins.map((b, i) => {
                  const total = bins.reduce((n, bin) => n + bin.decodes, 0);
                  const rate = rateOf(b);
                  return (
                    <tr key={i}>
                      <th>{binLabel(i, bins.length)}%</th>
                      <td>{b.decodes.toLocaleString()}</td>
                      <td>{total === 0 ? "—" : percent(b.decodes / total)}</td>
                      <td>{b.opportunities.toLocaleString()}</td>
                      <td>{b.heard.toLocaleString()}</td>
                      <td>{rate === null ? "—" : percent(rate)}</td>
                    </tr>
                  );
                })}
              </tbody>
            </table>
          </details>
          <p className="hint">
            {report.decodesSkipped.toLocaleString()} decodes skipped: no locator, a band outside
            the model, or a month outside the sunspot table.
          </p>
        </>
      )}
    </section>
  );
}
