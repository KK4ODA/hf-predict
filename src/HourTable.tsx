import { useState } from "react";
import { FrequencyPrediction, PathPrediction } from "./types";
import { clockHour, hourLabel, shade } from "./tiers";

type Metric = {
  key: string;
  label: string;
  value: (f: FrequencyPrediction) => number;
  format: (v: number) => string;
};

const METRICS: Metric[] = [
  {
    key: "rel",
    label: "Reliability (%)",
    value: (f) => f.reliability,
    format: (v) => (v * 100).toFixed(0),
  },
  { key: "snr", label: "SNR (dB-Hz)", value: (f) => f.snrDb, format: (v) => v.toFixed(0) },
  { key: "sdbw", label: "Signal (dBW)", value: (f) => f.signalDbw, format: (v) => v.toFixed(0) },
  {
    key: "mufday",
    label: "Days the band is open (%)",
    value: (f) => f.mufDay,
    format: (v) => (v * 100).toFixed(0),
  },
];

function details(f: FrequencyPrediction, requiredSnr: number): string {
  return [
    `${f.freqMhz} MHz, mode ${f.mode}, take-off ${f.takeoffAngleDeg}°`,
    `Reliability ${(f.reliability * 100).toFixed(0)}% for ${requiredSnr} dB-Hz`,
    `SNR ${f.snrDb} dB-Hz (signal ${f.signalDbw} dBW, noise ${f.noiseDbw} dBW)`,
    `Antenna gain: transmit ${f.txGainDbi} dBi, receive ${f.rxGainDbi} dBi`,
    `Path supports this frequency on ${(f.mufDay * 100).toFixed(0)}% of days`,
  ].join("\n");
}

type Props = {
  result: PathPrediction;
  hourIndex: number;
  onSelectHour: (hourIndex: number) => void;
};

/** Every hour against every band. Clicking a row selects that hour. */
export function HourTable({ result, hourIndex, onSelectHour }: Props) {
  const [metricKey, setMetricKey] = useState("rel");
  const metric = METRICS.find((m) => m.key === metricKey) ?? METRICS[0];
  const hours = result.run.prediction.hours
    .map((hour, index) => ({ hour, index }))
    .sort((a, b) => clockHour(a.hour.utcHour) - clockHour(b.hour.utcHour));

  return (
    <section>
      <h3>Hour by band</h3>
      <label className="inline">
        Show
        <select value={metricKey} onChange={(e) => setMetricKey(e.target.value)}>
          {METRICS.map((m) => (
            <option key={m.key} value={m.key}>
              {m.label}
            </option>
          ))}
        </select>
      </label>
      <table className="results">
        <thead>
          <tr>
            <th>UTC</th>
            <th>MUF</th>
            {result.bands.map((band) => (
              <th key={band.name}>{band.name}</th>
            ))}
          </tr>
        </thead>
        <tbody>
          {hours.map(({ hour, index }) => (
            <tr
              key={hour.utcHour}
              className={index === hourIndex ? "selected" : undefined}
              onClick={() => onSelectHour(index)}
            >
              <th>{hourLabel(hour.utcHour)}</th>
              <td>{hour.mufMhz.toFixed(1)}</td>
              {hour.frequencies.map((f) => (
                <td
                  key={f.freqMhz}
                  style={{ background: shade(f.reliability) }}
                  title={details(f, result.requiredSnrDbHz)}
                >
                  {metric.format(metric.value(f))}
                </td>
              ))}
            </tr>
          ))}
        </tbody>
      </table>
      <p className="note">
        Shading shows reliability whatever value is displayed. Click a row to see that hour above.
        Hover over a cell for details.
      </p>
    </section>
  );
}
