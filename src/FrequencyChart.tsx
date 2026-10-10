import { useState } from "react";
import { Band, FrequencyWindow } from "./types";
import { clockHour, hourLabel } from "./tiers";
import { hourBoth, localHour, Zone } from "./localtime";

type Props = {
  window: FrequencyWindow[];
  bands: Band[];
  /** Index into `window` of the hour the rest of the screen is showing. */
  hourIndex: number;
  zone: Zone;
};

type Series = {
  key: "muf" | "fot" | "luf";
  label: string;
  short: string;
  value: (w: FrequencyWindow) => number | null;
};

const SERIES: Series[] = [
  { key: "muf", label: "MUF (half of days)", short: "MUF", value: (w) => w.mufMhz },
  { key: "fot", label: "Optimum (90% of days)", short: "Optimum", value: (w) => w.fotMhz },
  { key: "luf", label: "Lowest usable", short: "Lowest usable", value: (w) => w.lufMhz },
];

const WIDTH = 760;
const HEIGHT = 314;
const MARGIN = { top: 22, right: 52, bottom: 44, left: 40 };
const PLOT_W = WIDTH - MARGIN.left - MARGIN.right;
const PLOT_H = HEIGHT - MARGIN.top - MARGIN.bottom;
const MHZ_TICKS = [5, 10, 15, 20, 25, 30];

/** MUF, optimum working frequency and lowest usable frequency through the day. */
export function FrequencyChart({ window, bands, hourIndex, zone }: Props) {
  const [hover, setHover] = useState<number | null>(null);
  const hours = [...window].sort((a, b) => clockHour(a.utcHour) - clockHour(b.utcHour));
  const top = Math.max(32, ...hours.map((w) => w.mufMhz));
  const x = (clock: number) => MARGIN.left + (clock / 23) * PLOT_W;
  const y = (mhz: number) => MARGIN.top + PLOT_H - (mhz / top) * PLOT_H;

  // A missing value breaks the line instead of being drawn as zero.
  const path = (series: Series) =>
    hours
      .map((w, i) => {
        const value = series.value(w);
        if (value === null) return "";
        const previous = i > 0 ? series.value(hours[i - 1]) : null;
        return `${previous === null ? "M" : "L"}${x(i).toFixed(1)},${y(value).toFixed(1)}`;
      })
      .join(" ");

  const peak = hours.reduce((best, w, i) => (w.mufMhz > hours[best].mufMhz ? i : best), 0);
  const selectedClock = clockHour(window[hourIndex].utcHour);
  const shown = hover !== null ? hours[hover] : null;

  return (
    <section>
      <h3>Usable frequencies through the day</h3>
      <div className="legend">
        {SERIES.map((s) => (
          <span key={s.key}>
            <span className={`key key-${s.key}`} /> {s.label}
          </span>
        ))}
      </div>
      <div className="chart">
        <svg
          viewBox={`0 0 ${WIDTH} ${HEIGHT}`}
          role="img"
          aria-label="Line chart of maximum usable, optimum and lowest usable frequency by UTC hour"
          onPointerMove={(event) => {
            const box = event.currentTarget.getBoundingClientRect();
            const px = ((event.clientX - box.left) / box.width) * WIDTH;
            const clock = Math.round(((px - MARGIN.left) / PLOT_W) * 23);
            setHover(Math.min(23, Math.max(0, clock)));
          }}
          onPointerLeave={() => setHover(null)}
        >
          {bands.map((band) => (
            <g key={band.name}>
              <line className="grid" x1={MARGIN.left} x2={MARGIN.left + PLOT_W} y1={y(band.mhz)} y2={y(band.mhz)} />
              <text className="tick" x={MARGIN.left + PLOT_W + 6} y={y(band.mhz) + 3.5}>
                {band.name}
              </text>
            </g>
          ))}
          {MHZ_TICKS.map((mhz) => (
            <text key={mhz} className="tick" x={MARGIN.left - 6} y={y(mhz) + 3.5} textAnchor="end">
              {mhz}
            </text>
          ))}
          <text className="tick" x={MARGIN.left - 6} y={MARGIN.top - 3} textAnchor="end">
            MHz
          </text>
          <line className="axis" x1={MARGIN.left} x2={MARGIN.left + PLOT_W} y1={y(0)} y2={y(0)} />
          {hours.map(
            (w, i) =>
              i % 3 === 0 && (
                <g key={w.utcHour}>
                  <text className="tick" x={x(i)} y={HEIGHT - 24} textAnchor="middle">
                    {hourLabel(w.utcHour)}
                  </text>
                  <text className="tick" x={x(i)} y={HEIGHT - 8} textAnchor="middle">
                    {localHour(clockHour(w.utcHour), zone)}
                  </text>
                </g>
              ),
          )}
          <text className="tick" x={MARGIN.left + PLOT_W} y={HEIGHT - 24} textAnchor="end" dx={48}>
            UTC
          </text>
          <text className="tick" x={MARGIN.left + PLOT_W} y={HEIGHT - 8} textAnchor="end" dx={48}>
            {zone.name}
          </text>

          <line className="selected-hour" x1={x(selectedClock)} x2={x(selectedClock)} y1={MARGIN.top} y2={y(0)} />
          {SERIES.map((s) => (
            <path key={s.key} className={`line line-${s.key}`} d={path(s)} />
          ))}
          {SERIES.map((s) => {
            // Each line is named at the MUF peak, where the lines are furthest
            // apart, so colour is not the only cue.
            const value = s.value(hours[peak]);
            return (
              value !== null && (
                <text key={s.key} className="line-label" x={x(peak)} y={y(value) - 7} textAnchor="middle">
                  {s.short}
                </text>
              )
            );
          })}
          {hover !== null && (
            <g>
              <line className="crosshair" x1={x(hover)} x2={x(hover)} y1={MARGIN.top} y2={y(0)} />
              {SERIES.map((s) => {
                const value = s.value(hours[hover]);
                return (
                  value !== null && (
                    <circle key={s.key} className={`dot dot-${s.key}`} cx={x(hover)} cy={y(value)} r={4} />
                  )
                );
              })}
            </g>
          )}
        </svg>
        {shown && hover !== null && (
          <div
            className="tooltip"
            style={{
              left: `${(x(hover) / WIDTH) * 100}%`,
              transform: hover > 15 ? "translateX(calc(-100% - 12px))" : "translateX(12px)",
            }}
          >
            <div className="tooltip-title">{hourBoth(clockHour(shown.utcHour), zone)}</div>
            {SERIES.map((s) => {
              const value = s.value(shown);
              return (
                <div key={s.key}>
                  <span className={`key key-${s.key}`} />{" "}
                  <strong>{value === null ? "none" : `${value.toFixed(1)} MHz`}</strong> {s.label}
                </div>
              );
            })}
          </div>
        )}
      </div>
      <p className="note">
        Bands between the lowest usable and optimum lines are the dependable choices. Above the
        optimum line a band opens on fewer days; above the MUF, on fewer than half. The lowest
        usable frequency is where the median SNR first meets the mode's requirement; a gap means
        no frequency does at that hour.
      </p>
      <details>
        <summary>Show as table</summary>
        <table className="results">
          <thead>
            <tr>
              <th>UTC</th>
              <th>{zone.name}</th>
              {SERIES.map((s) => (
                <th key={s.key}>{s.label} (MHz)</th>
              ))}
            </tr>
          </thead>
          <tbody>
            {hours.map((w) => (
              <tr key={w.utcHour}>
                <th>{hourLabel(w.utcHour)}</th>
                <th>{localHour(clockHour(w.utcHour), zone)}</th>
                {SERIES.map((s) => (
                  <td key={s.key}>{s.value(w)?.toFixed(1) ?? "—"}</td>
                ))}
              </tr>
            ))}
          </tbody>
        </table>
      </details>
    </section>
  );
}
