import { hourBoth, Zone } from "./localtime";
import { useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { WorldMap } from "./WorldMap";
import { RelScale, Ring } from "./ui";
import { Band, Coverage, HeardStation, LatLon, Mode, StationProfile, HearingStation, HearingYourArea } from "./types";

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
  zone: Zone;
  onPickTx: (position: string) => void;
  onPickRx: (position: string) => void;
};

type CoverageState =
  | { kind: "idle" }
  /** `data` is the quick coarse map, shown while the fine one is computed. */
  | { kind: "running"; data?: Coverage }
  | { kind: "done"; data: Coverage }
  | { kind: "failed"; error: string };

type Progress = { done: number; total: number };

/** "every 4° of latitude and longitude", or the two steps when they differ. */
function spacing(data: Coverage): string {
  return data.latStepDeg === data.lonStepDeg
    ? `every ${data.latStepDeg}° of latitude and longitude`
    : `every ${data.latStepDeg}° of latitude and ${data.lonStepDeg}° of longitude`;
}

const DEFAULT_BAND = "20 m";
const HEARD_POLL_MS = 5000;
const HEARD_WINDOWS = [
  { minutes: 0, label: "no heard stations" },
  { minutes: 15, label: "heard in 15 minutes" },
  { minutes: 60, label: "heard in the hour" },
  { minutes: 360, label: "heard in 6 hours" },
  { minutes: 1440, label: "heard in 24 hours" },
];

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

/** Stations heard on one band, refreshed while the layer is shown. */
function useHeard(minutes: number, band: string): [HeardStation[], string] {
  const [stations, setStations] = useState<HeardStation[]>([]);
  const [error, setError] = useState("");
  useEffect(() => {
    if (minutes === 0) {
      setStations([]);
      return;
    }
    let current = true;
    const poll = () =>
      invoke<HeardStation[]>("heard_stations", { minutes, band })
        .then((heard) => {
          if (!current) return;
          setStations(heard);
          setError("");
        })
        .catch((e) => current && setError(String(e)));
    poll();
    const timer = setInterval(poll, HEARD_POLL_MS);
    return () => {
      current = false;
      clearInterval(timer);
    };
  }, [minutes, band]);
  return [stations, error];
}

/** Stations heard reporting this one's area on one band, refreshed while shown. */
function useHearing(minutes: number, band: string, receiver: string): HearingStation[] {
  const [stations, setStations] = useState<HearingStation[]>([]);
  useEffect(() => {
    if (minutes === 0) {
      setStations([]);
      return;
    }
    let current = true;
    const poll = () =>
      invoke<HearingYourArea>("hearing_your_area", { minutes, band, receiver })
        .then((found) => current && setStations(found.stations))
        .catch(() => current && setStations([]));
    poll();
    const timer = setInterval(poll, HEARD_POLL_MS * 3);
    return () => {
      current = false;
      clearInterval(timer);
    };
  }, [minutes, band, receiver]);
  return stations;
}

/** The map: picking either end of the path, predicted coverage, heard stations. */
export function MapPanel(props: Props) {
  const { txPosition, rxPosition, year, month, ssn, txStation, rxStation, mode, reliability, clockHour, zone } =
    props;
  const from = useResolved(txPosition);
  const to = useResolved(rxPosition);
  const [picking, setPicking] = useState<"from" | "to" | null>(null);
  const [bandIndex, setBandIndex] = useState(
    Math.max(0, props.bands.findIndex((b) => b.name === DEFAULT_BAND)),
  );
  const band = props.bands[bandIndex];
  const [coverage, setCoverage] = useState<CoverageState>({ kind: "idle" });
  const [progress, setProgress] = useState<Progress | null>(null);
  // Results only land if no newer request was made; `wanted` is whether the
  // operator asked to see coverage, so a kept map for new inputs shows at once.
  const generation = useRef(0);
  const wanted = useRef(false);
  const [heardMinutes, setHeardMinutes] = useState(60);
  const [heard, heardError] = useHeard(heardMinutes, band.name);
  const [showHearing, setShowHearing] = useState(true);
  const hearing = useHearing(showHearing ? heardMinutes : 0, band.name, txPosition);

  const request = useMemo(
    () => ({
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
    }),
    [txPosition, year, month, ssn, txStation, rxStation, mode, reliability, clockHour],
  );

  useEffect(() => {
    let active = true;
    const unlisten = listen<Progress>("coverage-progress", (event) => {
      if (active) setProgress(event.payload);
    });
    return () => {
      active = false;
      unlisten.then((stop) => stop()).catch(() => {});
    };
  }, []);

  // A coverage map describes the inputs it was computed from. When they
  // change it is dropped, unless a fine map for the new ones was kept.
  useEffect(() => {
    const current = ++generation.current;
    setCoverage({ kind: "idle" });
    if (!wanted.current) return;
    invoke<Coverage | null>("cached_coverage", { request })
      .then((kept) => kept && current === generation.current && setCoverage({ kind: "done", data: kept }))
      .catch(() => {});
  }, [request]);

  async function showCoverage() {
    const current = ++generation.current;
    const isCurrent = () => current === generation.current;
    wanted.current = true;
    setProgress(null);
    setCoverage({ kind: "running" });
    try {
      const kept = await invoke<Coverage | null>("cached_coverage", { request });
      if (!isCurrent()) return;
      if (kept) {
        setCoverage({ kind: "done", data: kept });
        return;
      }
      const coarse = await invoke<Coverage>("predict_coverage", { request, fine: false });
      if (!isCurrent()) return;
      setCoverage({ kind: "running", data: coarse });
      const fine = await invoke<Coverage>("predict_coverage", { request, fine: true });
      if (isCurrent()) setCoverage({ kind: "done", data: fine });
    } catch (error) {
      if (isCurrent()) setCoverage({ kind: "failed", error: String(error) });
    }
  }

  function hideCoverage() {
    generation.current++;
    wanted.current = false;
    setCoverage({ kind: "idle" });
  }

  const shown = coverage.kind === "done" || coverage.kind === "running" ? coverage.data : undefined;

  function pick(position: LatLon) {
    const text = `${position.lat.toFixed(2)}, ${position.lon.toFixed(2)}`;
    if (picking === "from") props.onPickTx(text);
    if (picking === "to") props.onPickRx(text);
    setPicking(null);
  }

  return (
    <section className="full">
      <div className="view-head">
        <h2>Map</h2>
        <span className="hint">Day and night at {hourBoth(clockHour, zone)}, mid-month</span>
      </div>

      <div className="controls">
        <div className="segmented" role="group" aria-label="Set a position by clicking the map">
          <button type="button" aria-pressed={picking === "from"} onClick={() => setPicking(picking === "from" ? null : "from")}>
            Set From on map
          </button>
          <button type="button" aria-pressed={picking === "to"} onClick={() => setPicking(picking === "to" ? null : "to")}>
            Set To on map
          </button>
        </div>
        <label className="inline">
          Band
          <select value={bandIndex} onChange={(e) => setBandIndex(Number(e.target.value))}>
            {props.bands.map((b, i) => (
              <option key={b.name} value={i}>
                {b.name}
              </option>
            ))}
          </select>
        </label>
        {coverage.kind === "done" ? (
          <button type="button" onClick={hideCoverage}>
            Hide coverage
          </button>
        ) : (
          <button type="button" onClick={showCoverage} disabled={!from || coverage.kind === "running"}>
            {coverage.kind === "running" ? "Computing coverage…" : "Show predicted coverage"}
          </button>
        )}
        <select value={heardMinutes} onChange={(e) => setHeardMinutes(Number(e.target.value))} aria-label="Heard stations layer">
          {HEARD_WINDOWS.map((w) => (
            <option key={w.minutes} value={w.minutes}>
              {w.label}
            </option>
          ))}
        </select>
        <label className="inline">
          <input
            type="checkbox"
            checked={showHearing}
            disabled={heardMinutes === 0}
            onChange={(e) => setShowHearing(e.target.checked)}
          />
          Stations hearing your area
        </label>
      </div>
      {picking && <p className="note">Click the map to set the {picking === "from" ? "From" : "To"} position. Press the button again to cancel.</p>}
      {!from && <p className="note">Enter a From position, or set it on the map, to compute coverage.</p>}
      {coverage.kind === "running" && coverage.data && (
        <p className="note">
          A quick map, {spacing(coverage.data)}, is shown while the detailed one is computed
          {progress && progress.total > 0 && progress.done < progress.total
            ? ` (${Math.round((100 * progress.done) / progress.total)}%)`
            : ""}
          . The detailed map is kept, so this hour shows at once next time.
        </p>
      )}
      {coverage.kind === "failed" && <p className="error">Coverage could not be computed: {coverage.error}</p>}
      {heardError && <p className="error">Heard stations could not be read: {heardError}</p>}

      <WorldMap
        from={from}
        to={to}
        longPath={props.longPath}
        month={month}
        clockHour={clockHour}
        coverage={shown ? { data: shown, bandIndex } : null}
        heard={heard}
        hearing={hearing}
        picking={picking !== null}
        onPick={pick}
      />

      <div className="maplegend">
        <span>
          <span className="swatch line" /> {props.longPath ? "Long" : "Short"} path
        </span>
        {shown && <RelScale label={`Predicted reach on ${band.name}`} />}
        {heardMinutes > 0 && (
          <span>
            <span className="swatch meas dot" /> {heard.length} {heard.length === 1 ? "station" : "stations"} heard on{" "}
            {band.name}
          </span>
        )}
        {heardMinutes > 0 && showHearing && (
          <span>
            <Ring /> {hearing.length} hearing your area
          </span>
        )}
        <span>
          <span className="swatch night" /> Night
        </span>
        <span className="hint">Grid lines mark Maidenhead fields. Drag to pan, scroll to zoom.</span>
      </div>
      {shown && (
        <p className="note">
          Shading: predicted reliability of reaching a station like "{rxStation.name}" from the From
          position on {band.name} at {hourBoth(clockHour, zone)}. It is predicted {spacing(shown)}, with
          antennas aimed to within 22.5° of each point, and shaded smoothly between the points; hover to
          read the nearest one.
        </p>
      )}
      {heardMinutes > 0 && (
        <p className="note">
          Dots: stations this receiver decoded on {band.name}, at the centre of the locator each sent.
          They show what was heard from here, which depends on who was transmitting; the shading
          predicts where a signal from here would be heard.
          {showHearing &&
            " Rings: stations heard sending a signal report to you or to a station near you, so they hear your area. A dot inside a ring was heard both ways."}
        </p>
      )}
    </section>
  );
}
