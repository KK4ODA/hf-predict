import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { WorldMap } from "./WorldMap";
import { Band, Coverage, LatLon, Mode, StationProfile } from "./types";

type Props = {
  txPosition: string;
  rxPosition: string;
  longPath: boolean;
  year: number;
  month: number;
  ssn: number | null;
  txStation: StationProfile;
  rxStation: StationProfile;
  mode: Mode;
  reliability: number;
  bands: Band[];
  /** UTC hour shown across the app, 0 to 23. */
  clockHour: number;
  onPickTx: (position: string) => void;
  onPickRx: (position: string) => void;
};

type CoverageState =
  | { kind: "idle" }
  | { kind: "running" }
  | { kind: "done"; data: Coverage }
  | { kind: "failed"; error: string };

const DEFAULT_BAND = "20 m";

/** Looks up a typed position; `null` while it is not a valid one. */
function useResolved(text: string): LatLon | null {
  const [position, setPosition] = useState<LatLon | null>(null);
  useEffect(() => {
    let current = true;
    invoke<LatLon>("resolve_position", { text })
      .then((resolved) => current && setPosition(resolved))
      .catch(() => current && setPosition(null));
    return () => {
      current = false;
    };
  }, [text]);
  return position;
}

/** The map, with picking of either end of the path and a coverage overlay. */
export function MapPanel(props: Props) {
  const { txPosition, rxPosition, year, month, ssn, txStation, rxStation, mode, reliability, clockHour } = props;
  const from = useResolved(txPosition);
  const to = useResolved(rxPosition);
  const [picking, setPicking] = useState<"from" | "to" | null>(null);
  const [bandIndex, setBandIndex] = useState(
    Math.max(0, props.bands.findIndex((b) => b.name === DEFAULT_BAND)),
  );
  const [coverage, setCoverage] = useState<CoverageState>({ kind: "idle" });

  // A coverage map describes the inputs it was computed from; drop it when they change.
  useEffect(() => {
    setCoverage({ kind: "idle" });
  }, [txPosition, year, month, ssn, txStation, rxStation, mode, reliability, clockHour]);

  async function showCoverage() {
    setCoverage({ kind: "running" });
    try {
      const data = await invoke<Coverage>("predict_coverage", {
        request: {
          txPosition,
          year,
          month,
          ssn,
          txStation,
          rxStation,
          mode,
          requiredReliabilityPct: reliability,
          // The engine numbers hours 1-24, where 24 is 00 UTC.
          utcHour: clockHour === 0 ? 24 : clockHour,
        },
      });
      setCoverage({ kind: "done", data });
    } catch (error) {
      setCoverage({ kind: "failed", error: String(error) });
    }
  }

  function pick(position: LatLon) {
    const text = `${position.lat.toFixed(2)}, ${position.lon.toFixed(2)}`;
    if (picking === "from") props.onPickTx(text);
    if (picking === "to") props.onPickRx(text);
    setPicking(null);
  }

  return (
    <section>
      <div className="controls">
        <div className="segmented" role="group" aria-label="Pick a location on the map">
          <button
            type="button"
            aria-pressed={picking === "from"}
            onClick={() => setPicking(picking === "from" ? null : "from")}
          >
            Pick From
          </button>
          <button
            type="button"
            aria-pressed={picking === "to"}
            onClick={() => setPicking(picking === "to" ? null : "to")}
          >
            Pick To
          </button>
        </div>
        <label className="inline">
          Band
          <select value={bandIndex} onChange={(e) => setBandIndex(Number(e.target.value))}>
            {props.bands.map((band, i) => (
              <option key={band.name} value={i}>
                {band.name}
              </option>
            ))}
          </select>
        </label>
        <button type="button" onClick={showCoverage} disabled={!from || coverage.kind === "running"}>
          {coverage.kind === "running" ? "Computing…" : "Show coverage"}
        </button>
      </div>
      {picking && <p className="note">Click the map to set the {picking === "from" ? "From" : "To"} position.</p>}
      {coverage.kind === "failed" && <p className="error">Coverage failed: {coverage.error}</p>}

      <WorldMap
        from={from}
        to={to}
        longPath={props.longPath}
        month={month}
        clockHour={clockHour}
        coverage={coverage.kind === "done" ? { data: coverage.data, bandIndex } : null}
        picking={picking !== null}
        onPick={pick}
      />

      {coverage.kind === "done" && (
        <>
          <div className="ramp">
            <span>0%</span>
            <span className="ramp-bar" />
            <span>100% reliability</span>
          </div>
          <p className="note">
            Reliability of reaching a station like "{rxStation.name}" from the From position on{" "}
            {props.bands[bandIndex].name} at {String(clockHour).padStart(2, "0")} UTC, in{" "}
            {coverage.data.latStepDeg}° by {coverage.data.lonStepDeg}° cells. Antennas are aimed
            to within 22.5° of each cell. Hover over a cell for its values.
          </p>
        </>
      )}
      <p className="note">
        The shaded half is night at the chosen hour, mid-month. Grid lines mark Maidenhead fields.
      </p>
    </section>
  );
}
