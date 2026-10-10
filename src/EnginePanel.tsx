import { useState } from "react";
import { PathDetail } from "./types";

const SSN_KIND = {
  observed: "observed",
  predicted: "predicted",
  manual: "entered by hand",
};

/** What the engine was given and what it returned, for checking by hand. */
export function EnginePanel({ detail }: { detail: PathDetail }) {
  const prediction = detail.prediction;
  const [copied, setCopied] = useState("");
  const copy = (what: string, text: string) =>
    navigator.clipboard.writeText(text).then(
      () => setCopied(what),
      () => setCopied(""),
    );

  return (
    <section>
      <div className="view-head">
        <h2>Engine</h2>
        <span className="hint">The exact run behind the prediction on screen</span>
      </div>
      <div className="panel" style={{ maxWidth: 760 }}>
        <dl>
          <dt>Engine</dt>
          <dd>
            {prediction.run.prediction.engine}, run by {prediction.engine}
          </dd>
          <dt>Path</dt>
          <dd>
            {prediction.txLocator} to {prediction.rxLocator}, {prediction.distanceKm.toFixed(0)} km, bearing{" "}
            {prediction.txBearingDeg.toFixed(0)}° out and {prediction.rxBearingDeg.toFixed(0)}° back
          </dd>
          <dt>Sunspot number</dt>
          <dd>
            {prediction.ssn.value}, {SSN_KIND[prediction.ssn.kind]}
            {prediction.ssn.tableGenerated && `, from the table of ${prediction.ssn.tableGenerated}`}
          </dd>
          <dt>Required SNR</dt>
          <dd>{prediction.requiredSnrDbHz} dB-Hz</dd>
          <dt>Antennas</dt>
          <dd>Aimed along the path at both ends</dd>
          <dt>Made by</dt>
          <dd>
            VOACAP, developed by NTIA/ITS for the Voice of America from IONCAP; built from voacapl, the
            gfortran port by Jim Watson, HZ1JW. Full credits are under Stations.
          </dd>
          <dt>Kind of prediction</dt>
          <dd>Monthly climatology: the share of days a path works, not a forecast for today</dd>
        </dl>
      </div>

      <details>
        <summary>Input deck</summary>
        <div className="controls">
          <button type="button" onClick={() => copy("input", prediction.run.input)}>
            Copy input deck
          </button>
          {copied === "input" && <span className="hint">Copied.</span>}
        </div>
        <pre>{prediction.run.input}</pre>
      </details>
      <details>
        <summary>Output</summary>
        <div className="controls">
          <button type="button" onClick={() => copy("output", prediction.run.output)}>
            Copy output
          </button>
          {copied === "output" && <span className="hint">Copied.</span>}
        </div>
        <pre>{prediction.run.output}</pre>
      </details>
    </section>
  );
}
