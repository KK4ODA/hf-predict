import { useState } from "react";
import { BestBands } from "./BestBands";
import { FrequencyChart } from "./FrequencyChart";
import { HourTable } from "./HourTable";
import { PowerTable } from "./PowerTable";
import { PathOverview } from "./types";
import { clockHour, hourLabel } from "./tiers";

const SSN_KIND = {
  observed: "observed",
  predicted: "predicted",
  manual: "entered by hand",
};

type Props = { overview: PathOverview; startOnLongPath: boolean };

/** The results screen: one path and one hour at a time, switchable. */
export function Overview({ overview, startOnLongPath }: Props) {
  const [longPath, setLongPath] = useState(startOnLongPath);
  const detail = longPath ? overview.long : overview.short;
  const other = longPath ? overview.short : overview.long;
  const result = detail.prediction;
  const hours = result.run.prediction.hours;

  const nowClock = new Date().getUTCHours();
  const nowIndex = Math.max(
    0,
    hours.findIndex((h) => clockHour(h.utcHour) === nowClock),
  );
  const [hourIndex, setHourIndex] = useState(nowIndex);
  const ssn = result.ssn;

  return (
    <section className="overview">
      <h2>
        {result.txLocator} → {result.rxLocator}
      </h2>
      <p className="summary">
        {result.distanceKm.toFixed(0)} km · bearing {result.txBearingDeg.toFixed(0)}° out,{" "}
        {result.rxBearingDeg.toFixed(0)}° back · required SNR {result.requiredSnrDbHz} dB-Hz
        <br />
        Sunspot number {ssn.value} ({SSN_KIND[ssn.kind]}
        {ssn.tableGenerated && `, ${ssn.source}, table of ${ssn.tableGenerated}`})
        <br />
        Climatological prediction for the month · {result.run.prediction.engine} via{" "}
        {result.engine}
      </p>

      <div className="controls">
        <div className="segmented" role="group" aria-label="Path">
          <button type="button" aria-pressed={!longPath} onClick={() => setLongPath(false)}>
            Short path
          </button>
          <button type="button" aria-pressed={longPath} onClick={() => setLongPath(true)}>
            Long path
          </button>
        </div>
        <label className="inline">
          Hour
          <select value={hourIndex} onChange={(e) => setHourIndex(Number(e.target.value))}>
            {hours
              .map((hour, index) => ({ hour, index }))
              .sort((a, b) => clockHour(a.hour.utcHour) - clockHour(b.hour.utcHour))
              .map(({ hour, index }) => (
                <option key={hour.utcHour} value={index}>
                  {hourLabel(hour.utcHour)} UTC{index === nowIndex ? " (now)" : ""}
                </option>
              ))}
          </select>
        </label>
      </div>

      <BestBands
        detail={detail}
        other={other}
        otherName={longPath ? "short" : "long"}
        hourIndex={hourIndex}
      />
      <FrequencyChart window={detail.window} bands={result.bands} hourIndex={hourIndex} />
      <HourTable result={result} hourIndex={hourIndex} onSelectHour={setHourIndex} />
      <PowerTable detail={detail} hourIndex={hourIndex} />

      <p className="note">Antennas are assumed to be aimed along the path.</p>
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
