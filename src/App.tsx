import { FormEvent, useEffect, useState } from "react";
import { getVersion } from "@tauri-apps/api/app";
import { invoke } from "@tauri-apps/api/core";
import { BestBands } from "./BestBands";
import { ComparePanel } from "./ComparePanel";
import { ConditionsPanel } from "./ConditionsPanel";
import { FieldPanel } from "./FieldPanel";
import { FrequencyChart } from "./FrequencyChart";
import { HeardPanel } from "./HeardPanel";
import { HistoryPanel } from "./HistoryPanel";
import { PlanPanel } from "./PlanPanel";
import { HourTable } from "./HourTable";
import { MapPanel } from "./MapPanel";
import { PowerTable } from "./PowerTable";
import { StationEditor } from "./StationEditor";
import { UpdateCheck } from "./UpdateCheck";
import { clockHour as clockOf } from "./tiers";
import { localHour, zoneForMonth } from "./localtime";
import {
  ClockCheck,
  Conditions,
  ListenerStatus,
  Mode,
  Options,
  PathOverview,
  PathRequest,
  SavedLocation,
  StationProfile,
  UserData,
} from "./types";
import "./App.css";

type RunState =
  | { kind: "idle" }
  | { kind: "running" }
  | { kind: "done"; result: PathOverview }
  | { kind: "failed"; error: string };

type Tab = "bands" | "compare" | "field" | "day" | "map" | "heard" | "plan" | "history" | "conditions" | "engine";

const CLOCK_POLL_MS = 15000;

const TABS: { id: Tab; label: string; needsResult: boolean }[] = [
  { id: "bands", label: "Best bands", needsResult: true },
  { id: "compare", label: "Compare", needsResult: true },
  { id: "field", label: "Field", needsResult: true },
  { id: "day", label: "Through the day", needsResult: true },
  { id: "map", label: "Map", needsResult: false },
  { id: "heard", label: "Heard", needsResult: false },
  { id: "plan", label: "Plan", needsResult: false },
  { id: "history", label: "History", needsResult: false },
  { id: "conditions", label: "Conditions", needsResult: false },
  { id: "engine", label: "Engine", needsResult: true },
];

const SSN_KIND = {
  observed: "observed",
  predicted: "predicted",
  manual: "entered by hand",
};

function App() {
  const [version, setVersion] = useState("");
  const [options, setOptions] = useState<Options | null>(null);
  const [userData, setUserData] = useState<UserData>({ locations: [], stations: [], logFiles: [] });
  const [conditions, setConditions] = useState<Conditions | null>(null);
  const [clock, setClock] = useState<ClockCheck | null>(null);
  const [loadError, setLoadError] = useState("");

  const now = new Date();
  const [txPosition, setTxPosition] = useState("");
  const [rxPosition, setRxPosition] = useState("");
  const [year, setYear] = useState(now.getUTCFullYear());
  const [month, setMonth] = useState(now.getUTCMonth() + 1);
  const [ssn, setSsn] = useState("");
  const [mode, setMode] = useState<Mode>("ssb");
  const [reliability, setReliability] = useState(90);
  const [txStation, setTxStation] = useState<StationProfile | null>(null);
  const [rxStation, setRxStation] = useState<StationProfile | null>(null);
  const [newLocation, setNewLocation] = useState<SavedLocation>({ name: "", position: "" });
  const [run, setRun] = useState<RunState>({ kind: "idle" });

  // What the results area is showing.
  const [tab, setTab] = useState<Tab>("map");
  const [longPath, setLongPath] = useState(false);
  const [clockHour, setClockHour] = useState(now.getUTCHours());
  const zone = zoneForMonth(year, month);

  useEffect(() => {
    getVersion().then(setVersion);
    invoke<Options>("options").then((loaded) => {
      setOptions(loaded);
      setTxStation(loaded.presets[0]);
      setRxStation(loaded.presets[0]);
    });
    invoke<UserData>("load_user_data")
      .then(setUserData)
      .catch((error) => setLoadError(`Saved data: ${error}`));
    invoke<Conditions>("conditions")
      .then(setConditions)
      .catch((error) => setLoadError(`Conditions: ${error}`));
  }, []);

  // The clock check from the listener, for a warning on every screen.
  useEffect(() => {
    let active = true;
    const poll = () =>
      invoke<ListenerStatus>("listener_status")
        .then((s) => active && setClock(s.state === "listening" ? (s.tracker?.clock ?? null) : null))
        .catch(() => active && setClock(null));
    poll();
    const timer = setInterval(poll, CLOCK_POLL_MS);
    return () => {
      active = false;
      clearInterval(timer);
    };
  }, []);

  async function saveUserData(next: UserData) {
    setUserData(next);
    try {
      await invoke("save_user_data", { data: next });
      setLoadError("");
    } catch (error) {
      setLoadError(`Saved data: ${error}`);
    }
  }

  function saveStation(station: StationProfile) {
    const others = userData.stations.filter((s) => s.name !== station.name);
    saveUserData({ ...userData, stations: [...others, station] });
  }

  function addLocation() {
    const location = { name: newLocation.name.trim(), position: newLocation.position.trim() };
    const others = userData.locations.filter((l) => l.name !== location.name);
    saveUserData({ ...userData, locations: [...others, location] });
    setNewLocation({ name: "", position: "" });
  }

  async function predict(event: FormEvent) {
    event.preventDefault();
    if (!txStation || !rxStation) return;
    const request: PathRequest = {
      txPosition,
      rxPosition,
      year,
      month,
      ssn: ssn.trim() === "" ? null : Number(ssn),
      txStation,
      rxStation,
      mode,
      requiredReliabilityPct: reliability,
      // Both paths are always computed; the switch above the results picks one.
      longPath: false,
    };
    setRun({ kind: "running" });
    try {
      const result = await invoke<PathOverview>("predict_overview", { request });
      setRun({ kind: "done", result });
      setTab((current) => (TABS.find((t) => t.id === current)?.needsResult ? current : "bands"));
    } catch (error) {
      setRun({ kind: "failed", error: String(error) });
    }
  }

  if (!options || !txStation || !rxStation) return <main className="app">Loading…</main>;

  const overview = run.kind === "done" ? run.result : null;
  const detail = overview && (longPath ? overview.long : overview.short);
  const other = overview && (longPath ? overview.short : overview.long);
  const hours = detail?.prediction.run.prediction.hours ?? [];
  const hourIndex = Math.max(0, hours.findIndex((h) => clockOf(h.utcHour) === clockHour));
  const selectHour = (index: number) => setClockHour(clockOf(hours[index].utcHour));
  const modeLabel = options.modes.find((m) => m.value === mode)?.label ?? mode;
  const staleData =
    conditions !== null &&
    (conditions.ssnTable.stale || conditions.products.some((p) => p.stale || !p.stored));

  return (
    <main className="app">
      <header className="titlebar">
        <h1>
          hf-predict <span className="version">{version}</span>
        </h1>
        <UpdateCheck />
      </header>
      {loadError && <p className="error">{loadError}</p>}

      <div className="layout">
        <aside className="sidebar">
          <form onSubmit={predict}>
            <fieldset>
              <legend>Path</legend>
              <label>
                From (locator or latitude, longitude)
                <input
                  required
                  list="saved-locations"
                  placeholder="EM73tr"
                  value={txPosition}
                  onChange={(e) => setTxPosition(e.target.value)}
                />
              </label>
              <label>
                To
                <input
                  required
                  list="saved-locations"
                  placeholder="51.51, -0.13"
                  value={rxPosition}
                  onChange={(e) => setRxPosition(e.target.value)}
                />
              </label>
              <datalist id="saved-locations">
                {userData.locations.map((l) => (
                  <option key={l.name} value={l.position}>
                    {l.name}
                  </option>
                ))}
              </datalist>
              <div className="row">
                <label>
                  Year
                  <input
                    type="number"
                    min={1990}
                    max={2100}
                    value={year}
                    onChange={(e) => setYear(Number(e.target.value))}
                  />
                </label>
                <label>
                  Month
                  <input
                    type="number"
                    min={1}
                    max={12}
                    value={month}
                    onChange={(e) => setMonth(Number(e.target.value))}
                  />
                </label>
              </div>
              <div className="row">
                <label>
                  Mode
                  <select value={mode} onChange={(e) => setMode(e.target.value as Mode)}>
                    {options.modes.map((m) => (
                      <option key={m.value} value={m.value}>
                        {m.label}
                      </option>
                    ))}
                  </select>
                </label>
                <label>
                  Reliability (%)
                  <input
                    type="number"
                    min={10}
                    max={99}
                    value={reliability}
                    onChange={(e) => setReliability(Number(e.target.value))}
                  />
                </label>
              </div>
              <label>
                Sunspot number (blank = table)
                <input
                  type="number"
                  min={0}
                  max={300}
                  step="any"
                  value={ssn}
                  onChange={(e) => setSsn(e.target.value)}
                />
              </label>
              <button className="primary" type="submit" disabled={run.kind === "running"}>
                {run.kind === "running" ? "Predicting…" : "Predict"}
              </button>
            </fieldset>

            <StationEditor
              title="My station (transmits)"
              station={txStation}
              options={options}
              saved={userData.stations}
              onChange={setTxStation}
              onSave={saveStation}
            />
            <StationEditor
              title="Other station (receives)"
              station={rxStation}
              options={options}
              saved={userData.stations}
              onChange={setRxStation}
              onSave={saveStation}
            />

            <fieldset>
              <legend>Saved locations</legend>
              {userData.locations.length === 0 && <p className="note">None yet.</p>}
              <ul className="locations">
                {userData.locations.map((l) => (
                  <li key={l.name}>
                    <span>
                      {l.name} <small>{l.position}</small>
                    </span>
                    <button type="button" onClick={() => setTxPosition(l.position)}>
                      From
                    </button>
                    <button type="button" onClick={() => setRxPosition(l.position)}>
                      To
                    </button>
                    <button
                      type="button"
                      onClick={() =>
                        saveUserData({
                          ...userData,
                          locations: userData.locations.filter((o) => o.name !== l.name),
                        })
                      }
                    >
                      Remove
                    </button>
                  </li>
                ))}
              </ul>
              <div className="row">
                <input
                  placeholder="Name"
                  value={newLocation.name}
                  onChange={(e) => setNewLocation({ ...newLocation, name: e.target.value })}
                />
                <input
                  placeholder="Locator or lat, lon"
                  value={newLocation.position}
                  onChange={(e) => setNewLocation({ ...newLocation, position: e.target.value })}
                />
                <button
                  type="button"
                  disabled={!newLocation.name.trim() || !newLocation.position.trim()}
                  onClick={addLocation}
                >
                  Add
                </button>
              </div>
            </fieldset>
          </form>
        </aside>

        <section className="content">
          {conditions?.storm && (
            <p className="banner" role="alert">
              ⚠ {conditions.storm}
            </p>
          )}
          {clock && (clock.level === "warn" || clock.level === "alarm") && (
            <p className="banner" role="alert">
              ⚠ This computer's clock looks off
              {clock.medianDtS !== null && ` by about ${Math.abs(clock.medianDtS).toFixed(1)} s`} (from{" "}
              {clock.samples} decodes). FT8 needs it within about a second, FT4 and FT2 tighter
              still; synchronise it, or decoding will suffer.
            </p>
          )}
          {run.kind === "failed" && <p className="error">Prediction failed: {run.error}</p>}

          {detail && (
            <div className="summary">
              <h2>
                {detail.prediction.txLocator} → {detail.prediction.rxLocator}
              </h2>
              <p>
                {detail.prediction.distanceKm.toFixed(0)} km · bearing{" "}
                {detail.prediction.txBearingDeg.toFixed(0)}° out,{" "}
                {detail.prediction.rxBearingDeg.toFixed(0)}° back · required SNR{" "}
                {detail.prediction.requiredSnrDbHz} dB-Hz · sunspot number{" "}
                {detail.prediction.ssn.value} ({SSN_KIND[detail.prediction.ssn.kind]}
                {detail.prediction.ssn.tableGenerated &&
                  `, table of ${detail.prediction.ssn.tableGenerated}`}
                ) · climatological prediction for the month
              </p>
            </div>
          )}

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
              <select value={clockHour} onChange={(e) => setClockHour(Number(e.target.value))}>
                {Array.from({ length: 24 }, (_, h) => (
                  <option key={h} value={h}>
                    {String(h).padStart(2, "0")} UTC · {localHour(h, zone)} {zone.name}
                    {h === now.getUTCHours() ? " (now)" : ""}
                  </option>
                ))}
              </select>
            </label>
          </div>

          <nav className="tabs" role="tablist">
            {TABS.map((t) => (
              <button
                key={t.id}
                type="button"
                role="tab"
                aria-selected={tab === t.id}
                onClick={() => setTab(t.id)}
              >
                {t.label}
                {t.id === "conditions" && staleData && (
                  <span className="dot" title="Some data is stale or missing">
                    {" "}
                    ◐
                  </span>
                )}
              </button>
            ))}
          </nav>

          {TABS.find((t) => t.id === tab)?.needsResult && !detail && (
            <p className="note">Enter a path on the left and press Predict.</p>
          )}

          {tab === "bands" && detail && other && (
            <>
              <BestBands
                detail={detail}
                other={other}
                otherName={longPath ? "short" : "long"}
                hourIndex={hourIndex}
                zone={zone}
              />
              <PowerTable detail={detail} hourIndex={hourIndex} zone={zone} />
            </>
          )}

          {tab === "compare" && detail && (
            <ComparePanel detail={detail} hourIndex={hourIndex} modeLabel={modeLabel} zone={zone} />
          )}

          {tab === "field" && detail && (
            <FieldPanel
              detail={detail}
              hourIndex={hourIndex}
              modeLabel={modeLabel}
              longPath={longPath}
              month={month}
              zone={zone}
              conditions={conditions}
            />
          )}

          {tab === "day" && detail && (
            <>
              <FrequencyChart
                window={detail.window}
                bands={detail.prediction.bands}
                hourIndex={hourIndex}
                zone={zone}
              />
              <HourTable
                result={detail.prediction}
                hourIndex={hourIndex}
                onSelectHour={selectHour}
                zone={zone}
              />
            </>
          )}

          {tab === "map" && (
            <MapPanel
              txPosition={txPosition}
              rxPosition={rxPosition}
              longPath={longPath}
              year={year}
              month={month}
              ssn={ssn.trim() === "" ? null : Number(ssn)}
              txStation={txStation}
              rxStation={rxStation}
              mode={mode}
              reliability={reliability}
              bands={options.bands}
              clockHour={clockHour}
              zone={zone}
              onPickTx={setTxPosition}
              onPickRx={setRxPosition}
            />
          )}

          {tab === "heard" && (
            <HeardPanel
              logFiles={userData.logFiles}
              onLogFilesChange={(logFiles) => saveUserData({ ...userData, logFiles })}
              defaultRxPosition={txPosition}
            />
          )}

          {tab === "plan" && (
            <PlanPanel
              txPosition={txPosition}
              rxPosition={rxPosition}
              year={year}
              month={month}
              ssn={ssn.trim() === "" ? null : Number(ssn)}
              txStation={txStation}
              rxStation={rxStation}
              clockHour={clockHour}
              zone={zone}
            />
          )}

          {tab === "history" && (
            <HistoryPanel rxPosition={txPosition} noiseDb={txStation?.noiseDb ?? null} />
          )}

          {tab === "conditions" &&
            (conditions ? (
              <ConditionsPanel conditions={conditions} onUpdate={setConditions} />
            ) : (
              <p className="note">Conditions could not be loaded.</p>
            ))}

          {tab === "engine" && detail && (
            <section>
              <p className="note">
                {detail.prediction.run.prediction.engine} via {detail.prediction.engine}. Antennas
                are assumed to be aimed along the path.
              </p>
              <h3>Input deck</h3>
              <pre>{detail.prediction.run.input}</pre>
              <h3>Output</h3>
              <pre>{detail.prediction.run.output}</pre>
            </section>
          )}
        </section>
      </div>
    </main>
  );
}

export default App;
