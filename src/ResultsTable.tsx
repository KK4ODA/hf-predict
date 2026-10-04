import { useState } from "react";
import { FrequencyPrediction, PathPrediction } from "./types";

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
    label: "Days band is below MUF (%)",
    value: (f) => f.mufDay,
    format: (v) => (v * 100).toFixed(0),
  },
];

const SSN_KIND = {
  observed: "observed",
  predicted: "predicted",
  manual: "entered by hand",
};

/** Cells are shaded by reliability whatever metric is shown. */
function shade(reliability: number): string {
  return `rgba(46, 160, 67, ${(reliability * 0.75).toFixed(2)})`;
}

function details(f: FrequencyPrediction, requiredSnr: number): string {
  return [
    `${f.freqMhz} MHz, mode ${f.mode}, take-off ${f.takeoffAngleDeg}°`,
    `Reliability ${(f.reliability * 100).toFixed(0)}% for ${requiredSnr} dB-Hz`,
    `SNR ${f.snrDb} dB-Hz (signal ${f.signalDbw} dBW, noise ${f.noiseDbw} dBW)`,
    `Antenna gain: transmit ${f.txGainDbi} dBi, receive ${f.rxGainDbi} dBi`,
    `Path supports this frequency on ${(f.mufDay * 100).toFixed(0)}% of days`,
  ].join("\n");
}

export function ResultsTable({ result }: { result: PathPrediction }) {
  const [metricKey, setMetricKey] = useState("rel");
  const metric = METRICS.find((m) => m.key === metricKey) ?? METRICS[0];
  const prediction = result.run.prediction;
  // VOACAP numbers hours 1-24; hour 24 is 00 UTC.
  const hours = [...prediction.hours].sort((a, b) => (a.utcHour % 24) - (b.utcHour % 24));
  const ssn = result.ssn;

  return (
    <section>
      <h2>
        {result.txLocator} → {result.rxLocator}
      </h2>
      <p className="summary">
        {prediction.distanceKm.toFixed(0)} km · bearing {prediction.azimuthTxDeg.toFixed(0)}° out,{" "}
        {prediction.azimuthRxDeg.toFixed(0)}° back · required SNR {result.requiredSnrDbHz} dB-Hz
        <br />
        Sunspot number {ssn.value} ({SSN_KIND[ssn.kind]}
        {ssn.tableGenerated && `, ${ssn.source}, table of ${ssn.tableGenerated}`})
        <br />
        Climatological prediction for the month · {prediction.engine} via {result.engine}
      </p>

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
          {hours.map((hour) => (
            <tr key={hour.utcHour}>
              <th>{String(hour.utcHour % 24).padStart(2, "0")}</th>
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
        Shading shows reliability: the share of days in the month on which the required SNR is
        met. Antennas are assumed to be aimed along the path. Hover over a cell for details.
      </p>

      <details>
        <summary>Engine input and output</summary>
        <h3>Input deck</h3>
        <pre>{result.run.input}</pre>
        <h3>Output</h3>
        <pre>{result.run.output}</pre>
      </details>
    </section>
  );
}
