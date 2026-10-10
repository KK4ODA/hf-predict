import { useState } from "react";
import { PathDetail } from "./types";
import { clockHour, hourLabel, TIER_LEGEND } from "./tiers";
import { hourBoth, localHour, Zone } from "./localtime";
import { pct, relBin, relFill, RelScale, relStyle, useWidth } from "./ui";

type Props = {
  detail: PathDetail;
  hourIndex: number;
  onSelectHour: (index: number) => void;
  zone: Zone;
  nowClock: number;
  modeLabel: string;
};

type CellText = "none" | "rel" | "snr" | "mufday";
const CELL_TEXT: { key: CellText; label: string }[] = [
  { key: "none", label: "nothing" },
  { key: "rel", label: "reliability %" },
  { key: "snr", label: "SNR, dB-Hz" },
  { key: "mufday", label: "days open %" },
];

type Line = { key: "muf" | "fot" | "luf"; label: string; long: string };
const LINES: Line[] = [
  { key: "muf", label: "MUF", long: "maximum usable frequency, half of days" },
  { key: "fot", label: "FOT", long: "optimum working frequency, 90% of days" },
  { key: "luf", label: "LUF", long: "lowest usable frequency for the mode" },
];

const LEFT = 52;
const RIGHT = 44;
const TOP = 24;
const ROW = 30;
const BOTTOM = 46;

/**
 * Every band against every hour: cells shaded by reliability, with the
 * usable-frequency lines threaded through the bands they fall between.
 */
export function BandLadder({ detail, hourIndex, onSelectHour, zone, nowClock, modeLabel }: Props) {
  const [text, setText] = useState<CellText>("none");
  const [hover, setHover] = useState<{ row: number; clock: number } | null>(null);
  const [box, width] = useWidth<HTMLDivElement>(980);
  const W = Math.max(640, Math.min(1500, width));
  const prediction = detail.prediction;
  const hours = prediction.run.prediction.hours;
  const indexOfClock = new Array<number>(24).fill(0);
  hours.forEach((h, i) => (indexOfClock[clockHour(h.utcHour)] = i));
  const windowAt = new Map(detail.window.map((w) => [clockHour(w.utcHour), w]));
  const lineValue = (key: Line["key"], clock: number) => {
    const w = windowAt.get(clock);
    if (!w) return null;
    return key === "muf" ? w.mufMhz : key === "fot" ? w.fotMhz : w.lufMhz;
  };

  // Highest band at the top, as on a frequency axis.
  const order = prediction.bands.map((band, index) => ({ band, index })).sort((a, b) => b.band.mhz - a.band.mhz);
  const plotW = W - LEFT - RIGHT;
  const colW = plotW / 24;
  const plotH = order.length * ROW;
  const H = TOP + plotH + BOTTOM;
  const x = (clock: number) => LEFT + clock * colW;
  const rowY = (row: number) => TOP + row * ROW;

  // Frequency to height, interpolated between the centres of the band rows.
  const centres = order.map((o, row) => ({ mhz: o.band.mhz, y: rowY(row) + ROW / 2 })).reverse();
  const fy = (mhz: number) => {
    const c = centres;
    const along = (a: (typeof c)[0], b: (typeof c)[0]) => a.y + ((mhz - a.mhz) / (b.mhz - a.mhz)) * (b.y - a.y);
    if (mhz <= c[0].mhz) return Math.min(TOP + plotH, along(c[0], c[1]));
    for (let k = 1; k < c.length; k++) if (mhz <= c[k].mhz) return along(c[k - 1], c[k]);
    return Math.max(TOP - 8, along(c[c.length - 2], c[c.length - 1]));
  };
  const linePath = (key: Line["key"]) => {
    let d = "";
    let pen = false;
    for (let clock = 0; clock < 24; clock++) {
      const v = lineValue(key, clock);
      if (v === null) {
        pen = false;
        continue;
      }
      d += `${pen ? "L" : "M"}${(x(clock) + colW / 2).toFixed(1)},${fy(v).toFixed(1)} `;
      pen = true;
    }
    return d;
  };
  const lastPoint = (key: Line["key"]) => {
    for (let clock = 23; clock >= 0; clock--) {
      const v = lineValue(key, clock);
      if (v !== null) return { clock, y: fy(v) };
    }
    return null;
  };
  // Right-hand line labels, pushed apart so they never sit on each other.
  const labels = LINES.map((l) => ({ l, p: lastPoint(l.key) }))
    .filter((e): e is { l: Line; p: { clock: number; y: number } } => e.p !== null)
    .sort((a, b) => a.p.y - b.p.y);
  for (let k = 1; k < labels.length; k++) {
    if (labels[k].p.y - labels[k - 1].p.y < 13) labels[k].p = { ...labels[k].p, y: labels[k - 1].p.y + 13 };
  }

  const noLuf = detail.window.every((w) => w.lufMhz === null);
  const selectedClock = clockHour(hours[hourIndex].utcHour);
  const cellValue = (row: number, clock: number) => hours[indexOfClock[clock]].frequencies[order[row].index];
  const shown = hover ? cellValue(hover.row, hover.clock) : null;
  const required = prediction.requiredSnrDbHz;

  const pointer = (event: React.PointerEvent<SVGSVGElement>) => {
    const box = event.currentTarget.getBoundingClientRect();
    const px = ((event.clientX - box.left) / box.width) * W;
    const py = ((event.clientY - box.top) / box.height) * H;
    const clock = Math.floor((px - LEFT) / colW);
    const row = Math.floor((py - TOP) / ROW);
    return clock >= 0 && clock < 24 && row >= 0 && row < order.length ? { row, clock } : null;
  };

  return (
    <section>
      <div className="view-head">
        <h2>Through the day</h2>
        <span className="hint">
          {modeLabel}, needs {required} dB-Hz. Click a column to show that hour everywhere.
        </span>
      </div>

      <div className="controls">
        <div className="legend" style={{ margin: 0 }}>
          <RelScale />
          {LINES.map((l) => (
            <span key={l.key} title={l.long}>
              <span
                className={`swatch line${l.key === "luf" ? " dotted" : ""}`}
                style={{ opacity: l.key === "fot" ? 0.75 : 1 }}
              />
              {l.label}
              {l.key === "luf" && noLuf && <span className="hint">none: no frequency meets the mode's need</span>}
            </span>
          ))}
        </div>
        <span className="spacer" />
        <label className="inline">
          Numbers in cells
          <select value={text} onChange={(e) => setText(e.target.value as CellText)}>
            {CELL_TEXT.map((t) => (
              <option key={t.key} value={t.key}>
                {t.label}
              </option>
            ))}
          </select>
        </label>
      </div>

      <div className="chart" ref={box} style={{ maxWidth: 1500 }}>
        <svg
          viewBox={`0 0 ${W} ${H}`}
          role="img"
          aria-label="Reliability for each band and UTC hour, with the maximum, optimum and lowest usable frequencies"
          onPointerMove={(e) => setHover(pointer(e))}
          onPointerLeave={() => setHover(null)}
          onClick={(e) => {
            const at = pointer(e as unknown as React.PointerEvent<SVGSVGElement>);
            if (at) onSelectHour(indexOfClock[at.clock]);
          }}
          style={{ cursor: "pointer" }}
        >
          <defs>
            <clipPath id="ladder-plot">
              <rect x={LEFT} y={TOP - 10} width={plotW} height={plotH + 10} />
            </clipPath>
          </defs>

          {order.map((o, row) => (
            <g key={o.band.name}>
              <text className="tick" x={LEFT - 10} y={rowY(row) + ROW / 2 + 4} textAnchor="end" style={{ fill: "var(--ink-2)", fontSize: 13, fontWeight: 600 }}>
                {o.band.name}
              </text>
              {Array.from({ length: 24 }, (_, clock) => {
                const f = cellValue(row, clock);
                const value =
                  text === "rel"
                    ? Math.round(f.reliability * 100)
                    : text === "snr"
                      ? Math.round(f.snrDb)
                      : text === "mufday"
                        ? Math.round(f.mufDay * 100)
                        : null;
                return (
                  <g key={clock}>
                    <rect
                      className="cell"
                      x={x(clock) + 1}
                      y={rowY(row) + 1}
                      width={colW - 2}
                      height={ROW - 2}
                      style={relFill(f.reliability)}
                    />
                    {value !== null && (
                      <text
                        className="cell-text"
                        x={x(clock) + colW / 2}
                        y={rowY(row) + ROW / 2 + 4}
                        textAnchor="middle"
                        style={{ fill: `var(--rel-ink-${relBin(f.reliability)})` }}
                      >
                        {value}
                      </text>
                    )}
                  </g>
                );
              })}
            </g>
          ))}

          <g clipPath="url(#ladder-plot)">
            {LINES.map((l) => (
              <path key={`h${l.key}`} className="halo" d={linePath(l.key)} />
            ))}
            {LINES.map((l) => (
              <path key={l.key} className={`line line-${l.key}`} d={linePath(l.key)} />
            ))}
          </g>
          {labels.map(({ l, p }) => (
            <text key={l.key} className="line-label" x={LEFT + plotW + 6} y={p.y + 4}>
              {l.label}
            </text>
          ))}

          <rect className="sel-col" x={x(selectedClock) + 0.5} y={TOP - 1} width={colW - 1} height={plotH + 2} />
          <path
            className="now-mark"
            d={`M${x(nowClock) + colW / 2 - 5},${TOP - 10} l10,0 l-5,6 z`}
          />
          <text className="tick" x={x(nowClock) + colW / 2} y={TOP - 13} textAnchor="middle">
            now
          </text>
          {hover && <rect className="hover-cell" x={x(hover.clock) + 1} y={rowY(hover.row) + 1} width={colW - 2} height={ROW - 2} />}

          {Array.from({ length: 24 }, (_, clock) =>
            clock % 2 === 0 ? (
              <g key={clock}>
                <text className="tick" x={x(clock) + colW / 2} y={TOP + plotH + 16} textAnchor="middle">
                  {String(clock).padStart(2, "0")}
                </text>
                <text className="tick" x={x(clock) + colW / 2} y={TOP + plotH + 32} textAnchor="middle" style={{ opacity: 0.75 }}>
                  {localHour(clock, zone)}
                </text>
              </g>
            ) : null,
          )}
          <text className="axis-title" x={LEFT - 10} y={TOP + plotH + 16} textAnchor="end">
            UTC
          </text>
          <text className="axis-title" x={LEFT - 10} y={TOP + plotH + 32} textAnchor="end">
            {zone.name}
          </text>
        </svg>

        {hover && shown && (
          <div
            className="tooltip"
            style={{
              left: `${((x(hover.clock) + colW) / W) * 100}%`,
              top: `${((rowY(hover.row) + ROW) / H) * 100}%`,
              transform: hover.clock > 15 ? `translateX(calc(-100% - ${colW + 8}px))` : "translateX(6px)",
            }}
          >
            <div className="tooltip-title">
              {order[hover.row].band.name}, {hourBoth(hover.clock, zone)}
            </div>
            <div>
              <strong>{pct(shown.reliability)}</strong> of days reliable
            </div>
            <div>
              <strong>{shown.snrDb.toFixed(0)}</strong> dB-Hz median SNR, needs {required}
            </div>
            <div>
              Open on <strong>{pct(shown.mufDay)}</strong> of days
            </div>
            <div className="hint">
              {shown.mode}, take-off {shown.takeoffAngleDeg.toFixed(0)}°, MUF{" "}
              {windowAt.get(hover.clock)?.mufMhz.toFixed(1) ?? "?"} MHz
            </div>
          </div>
        )}
      </div>

      <p className="note">
        Shading: the share of days in the month the path meets the {required} dB-Hz the mode needs.{" "}
        {TIER_LEGEND} Lines: the {LINES.map((l) => `${l.label}, ${l.long}`).join("; ")}. Bands below
        the FOT and above the LUF are the dependable choices. Between band rows a line's height is
        interpolated in frequency.
      </p>

      <details>
        <summary>Show as a table</summary>
        <div className="scroll-x">
          <table className="results">
            <thead>
              <tr>
                <th>UTC</th>
                <th>{zone.name}</th>
                <th>MUF</th>
                <th>FOT</th>
                <th>LUF</th>
                {order.map((o) => (
                  <th key={o.band.name}>{o.band.name}</th>
                ))}
              </tr>
            </thead>
            <tbody>
              {Array.from({ length: 24 }, (_, clock) => {
                const w = windowAt.get(clock);
                const h = hours[indexOfClock[clock]];
                return (
                  <tr
                    key={clock}
                    className={clock === selectedClock ? "selected" : undefined}
                    onClick={() => onSelectHour(indexOfClock[clock])}
                    style={{ cursor: "pointer" }}
                  >
                    <th>{hourLabel(clock)}</th>
                    <th className="hint">{localHour(clock, zone)}</th>
                    <td>{w ? w.mufMhz.toFixed(1) : "—"}</td>
                    <td>{w?.fotMhz?.toFixed(1) ?? "—"}</td>
                    <td>{w?.lufMhz?.toFixed(1) ?? "—"}</td>
                    {order.map((o) => {
                      const f = h.frequencies[o.index];
                      return (
                        <td key={o.band.name} className="cell" style={relStyle(f.reliability)}>
                          {Math.round(f.reliability * 100)}
                        </td>
                      );
                    })}
                  </tr>
                );
              })}
            </tbody>
          </table>
        </div>
        <p className="note">Frequencies in MHz; band columns are reliability in percent.</p>
      </details>
    </section>
  );
}
