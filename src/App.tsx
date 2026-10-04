import { FormEvent, useEffect, useState } from "react";
import { getVersion } from "@tauri-apps/api/app";
import { invoke } from "@tauri-apps/api/core";
import { ResultsTable } from "./ResultsTable";
import { StationEditor } from "./StationEditor";
import { UpdateCheck } from "./UpdateCheck";
import {
  Mode,
  Options,
  PathPrediction,
  PathRequest,
  SavedLocation,
  StationProfile,
  UserData,
} from "./types";
import "./App.css";

type RunState =
  | { kind: "idle" }
  | { kind: "running" }
  | { kind: "done"; result: PathPrediction }
  | { kind: "failed"; error: string };

function App() {
  const [version, setVersion] = useState("");
  const [options, setOptions] = useState<Options | null>(null);
  const [userData, setUserData] = useState<UserData>({ locations: [], stations: [] });
  const [loadError, setLoadError] = useState("");

  const now = new Date();
  const [txPosition, setTxPosition] = useState("");
  const [rxPosition, setRxPosition] = useState("");
  const [year, setYear] = useState(now.getUTCFullYear());
  const [month, setMonth] = useState(now.getUTCMonth() + 1);
  const [ssn, setSsn] = useState("");
  const [mode, setMode] = useState<Mode>("ssb");
  const [reliability, setReliability] = useState(90);
  const [longPath, setLongPath] = useState(false);
  const [txStation, setTxStation] = useState<StationProfile | null>(null);
  const [rxStation, setRxStation] = useState<StationProfile | null>(null);
  const [newLocation, setNewLocation] = useState<SavedLocation>({ name: "", position: "" });
  const [run, setRun] = useState<RunState>({ kind: "idle" });

  useEffect(() => {
    getVersion().then(setVersion);
    invoke<Options>("options").then((loaded) => {
      setOptions(loaded);
      setTxStation(loaded.presets[0]);
      setRxStation(loaded.presets[0]);
    });
    invoke<UserData>("load_user_data")
      .then(setUserData)
      .catch((error) => setLoadError(String(error)));
  }, []);

  async function saveUserData(next: UserData) {
    setUserData(next);
    try {
      await invoke("save_user_data", { data: next });
      setLoadError("");
    } catch (error) {
      setLoadError(String(error));
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
      longPath,
    };
    setRun({ kind: "running" });
    try {
      setRun({ kind: "done", result: await invoke<PathPrediction>("predict_path", { request }) });
    } catch (error) {
      setRun({ kind: "failed", error: String(error) });
    }
  }

  if (!options || !txStation || !rxStation) return <main>Loading…</main>;

  return (
    <main>
      <header>
        <h1>
          hf-predict <span className="version">{version}</span>
        </h1>
        <UpdateCheck />
      </header>
      {loadError && <p className="error">Saved data: {loadError}</p>}

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
          <label className="check">
            <input
              type="checkbox"
              checked={longPath}
              onChange={(e) => setLongPath(e.target.checked)}
            />
            Long path
          </label>
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
          <label>
            Sunspot number (blank = bundled table)
            <input
              type="number"
              min={0}
              max={300}
              step="any"
              value={ssn}
              onChange={(e) => setSsn(e.target.value)}
            />
          </label>
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
              Required reliability (%)
              <input
                type="number"
                min={10}
                max={99}
                value={reliability}
                onChange={(e) => setReliability(Number(e.target.value))}
              />
            </label>
          </div>
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
                      locations: userData.locations.filter((other) => other.name !== l.name),
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

        <button className="primary" type="submit" disabled={run.kind === "running"}>
          {run.kind === "running" ? "Predicting…" : "Predict"}
        </button>
      </form>

      {run.kind === "failed" && <p className="error">Prediction failed: {run.error}</p>}
      {run.kind === "done" && <ResultsTable result={run.result} />}
    </main>
  );
}

export default App;
