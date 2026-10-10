import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { getVersion } from "@tauri-apps/api/app";
import { invoke } from "@tauri-apps/api/core";
import { BandLadder } from "./BandLadder";
import { BestBands } from "./BestBands";
import { ComparePanel } from "./ComparePanel";
import { ConditionsPanel } from "./ConditionsPanel";
import { EnginePanel } from "./EnginePanel";
import { FieldPanel } from "./FieldPanel";
import { HeardPanel } from "./HeardPanel";
import { HistoryPanel } from "./HistoryPanel";
import { MapPanel } from "./MapPanel";
import { PlanPanel } from "./PlanPanel";
import { PowerTable } from "./PowerTable";
import { RadioPanel } from "./RadioPanel";
import { SetupPanel, Theme } from "./SetupPanel";
import { updateNoticeText, useUpdater } from "./UpdateCheck";
import { Live, StatusBar } from "./StatusBar";
import { useComparison, verdictIcon } from "./comparison";
import { clockHour as clockOf } from "./tiers";
import { localHour, zoneForMonth } from "./localtime";
import {
  Conditions,
  ListenerStatus,
  Mode,
  Options,
  PathDetail,
  PathOverview,
  PathRequest,
  RadioStatus,
  ScanStatus,
  StationProfile,
  UserData,
} from "./types";
import "./App.css";

type RunState =
  | { kind: "idle" }
  | { kind: "running" }
  | { kind: "done"; result: PathOverview; key: string }
  | { kind: "failed"; error: string };

export type View =
  | "field"
  | "plan"
  | "radio"
  | "bands"
  | "day"
  | "map"
  | "heard"
  | "compare"
  | "history"
  | "conditions"
  | "setup"
  | "engine";

type NavItem = { id: View; label: string; needsPath: boolean };

const NAV: { group: string; items: NavItem[] }[] = [
  {
    group: "Operate",
    items: [
      { id: "field", label: "Field", needsPath: true },
      { id: "plan", label: "Plan", needsPath: false },
      { id: "radio", label: "Radio", needsPath: false },
    ],
  },
  {
    group: "Model",
    items: [
      { id: "bands", label: "Best bands", needsPath: true },
      { id: "day", label: "Through the day", needsPath: true },
      { id: "map", label: "Map", needsPath: false },
    ],
  },
  {
    group: "Observe",
    items: [
      { id: "heard", label: "Heard", needsPath: false },
      { id: "compare", label: "Compare", needsPath: true },
      { id: "history", label: "History", needsPath: false },
    ],
  },
  { group: "Space weather", items: [{ id: "conditions", label: "Conditions", needsPath: false }] },
  {
    group: "Setup",
    items: [
      { id: "setup", label: "Stations", needsPath: false },
      { id: "engine", label: "Engine", needsPath: true },
    ],
  },
];
const VIEWS = NAV.flatMap((g) => g.items);

const LIVE_POLL_MS = 3000;
const SESSION_KEY = "hfp-session";

type Session = {
  txPosition: string;
  rxPosition: string;
  mode: Mode;
  view: View;
  longPath: boolean;
};

/** The last path and view, so the app reopens where it was. Per computer only. */
function loadSession(): Partial<Session> {
  try {
    return JSON.parse(localStorage.getItem(SESSION_KEY) ?? "{}");
  } catch {
    return {};
  }
}

function saveSession(session: Session) {
  try {
    localStorage.setItem(SESSION_KEY, JSON.stringify(session));
  } catch {
    // Private windows and locked-down profiles have no storage; nothing to do.
  }
}

/** Listener, radio and scan state, refreshed for the status strip. */
function useLive(): Live {
  const [live, setLive] = useState<Live>({ listener: null, radio: null, scan: null });
  useEffect(() => {
    let current = true;
    const poll = async () => {
      const [listener, radio, scan] = await Promise.allSettled([
        invoke<ListenerStatus>("listener_status"),
        invoke<RadioStatus>("radio_status"),
        invoke<ScanStatus>("scan_status"),
      ]);
      if (!current) return;
      setLive({
        listener: listener.status === "fulfilled" ? listener.value : null,
        radio: radio.status === "fulfilled" ? radio.value : null,
        scan: scan.status === "fulfilled" ? scan.value : null,
      });
    };
    poll();
    const timer = setInterval(poll, LIVE_POLL_MS);
    return () => {
      current = false;
      clearInterval(timer);
    };
  }, []);
  return live;
}

/** The current time in seconds, ticking once a second. */
function useNow(): number {
  const [now, setNow] = useState(() => Date.now() / 1000);
  useEffect(() => {
    const timer = setInterval(() => setNow(Date.now() / 1000), 1000);
    return () => clearInterval(timer);
  }, []);
  return now;
}

/** The combined recommendation for the current hour, for the status strip. */
function BestNow({ detail, nowClock, onOpen }: { detail: PathDetail; nowClock: number; onOpen: () => void }) {
  const hours = detail.prediction.run.prediction.hours;
  const index = Math.max(0, hours.findIndex((h) => clockOf(h.utcHour) === nowClock));
  const [rows] = useComparison(detail, index, 60);
  const best = [...rows].sort(
    (a, b) => a.verdict.priority - b.verdict.priority || b.modeReliability - a.modeReliability,
  )[0];
  return (
    <button type="button" className="sb-cell sb-best" onClick={onOpen} title="Best band now, from the model and what was heard">
      <span className="sb-label">Best now</span>
      {best ? (
        <>
          <span className={`verdict verdict-${best.verdict.priority}`} aria-hidden="true">
            {verdictIcon(best.verdict.priority)}
          </span>
          <span className="sb-value">{best.band}</span>
          <span className="sb-sub sb-opt1">{best.verdict.label.toLowerCase()}</span>
        </>
      ) : (
        <span className="sb-value small">…</span>
      )}
    </button>
  );
}

function App() {
  const session = useMemo(loadSession, []);
  const [version, setVersion] = useState("");
  const [options, setOptions] = useState<Options | null>(null);
  const [userData, setUserData] = useState<UserData>({ locations: [], stations: [], logFiles: [] });
  const [conditions, setConditions] = useState<Conditions | null>(null);
  const [loadError, setLoadError] = useState("");
  const [theme, setTheme] = useState<Theme>(
    () => (document.documentElement.dataset.theme as Theme | undefined) ?? "dark",
  );

  const started = useMemo(() => new Date(), []);
  const [txPosition, setTxPosition] = useState(session.txPosition ?? "");
  const [rxPosition, setRxPosition] = useState(session.rxPosition ?? "");
  const [year, setYear] = useState(started.getUTCFullYear());
  const [month, setMonth] = useState(started.getUTCMonth() + 1);
  const [ssn, setSsn] = useState("");
  const [mode, setMode] = useState<Mode>(session.mode ?? "ssb");
  const [reliability, setReliability] = useState(90);
  const [txStation, setTxStation] = useState<StationProfile | null>(null);
  const [rxStation, setRxStation] = useState<StationProfile | null>(null);
  const [run, setRun] = useState<RunState>({ kind: "idle" });
  const [moreOpen, setMoreOpen] = useState(false);

  const [view, setView] = useState<View>(session.view ?? "field");
  const [longPath, setLongPath] = useState(session.longPath ?? false);
  const [clockHour, setClockHour] = useState(started.getUTCHours());
  const zone = zoneForMonth(year, month);
  const live = useLive();
  const nowS = useNow();
  const updater = useUpdater();
  // The version whose notice the operator put off with Later, this session only.
  const [laterFor, setLaterFor] = useState<string | null>(null);
  const nowClock = new Date(nowS * 1000).getUTCHours();
  const fromRef = useRef<HTMLInputElement>(null);
  const autoRan = useRef(false);

  useEffect(() => {
    getVersion().then(setVersion);
    invoke<Options>("options").then((loaded) => {
      setOptions(loaded);
      setTxStation(loaded.presets[0]);
      setRxStation(loaded.presets[0]);
    });
    invoke<UserData>("load_user_data")
      .then(setUserData)
      .catch((error) => setLoadError(`Saved data could not be read: ${error}`));
    invoke<Conditions>("conditions")
      .then(setConditions)
      .catch((error) => setLoadError(`Conditions could not be read: ${error}`));
  }, []);

  useEffect(() => {
    saveSession({ txPosition, rxPosition, mode, view, longPath });
  }, [txPosition, rxPosition, mode, view, longPath]);

  useEffect(() => {
    document.documentElement.dataset.theme = theme;
    try {
      localStorage.setItem("hfp-theme", theme);
    } catch {
      // No storage: the theme lasts for this session only.
    }
  }, [theme]);

  async function saveUserData(next: UserData) {
    setUserData(next);
    try {
      await invoke("save_user_data", { data: next });
      setLoadError("");
    } catch (error) {
      setLoadError(`Saved data could not be written: ${error}`);
    }
  }

  const request: PathRequest | null =
    txStation && rxStation
      ? {
          txPosition,
          rxPosition,
          year,
          month,
          ssn: ssn.trim() === "" ? null : Number(ssn),
          txStation,
          rxStation,
          mode,
          requiredReliabilityPct: reliability,
          // Both paths are always computed; the path switch picks one.
          longPath: false,
        }
      : null;
  const requestKey = request ? JSON.stringify(request) : "";

  const predict = useCallback(async () => {
    if (!request || !request.txPosition.trim() || !request.rxPosition.trim()) return;
    setRun({ kind: "running" });
    try {
      const result = await invoke<PathOverview>("predict_overview", { request });
      setRun({ kind: "done", result, key: JSON.stringify(request) });
    } catch (error) {
      setRun({ kind: "failed", error: String(error) });
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [requestKey]);

  // Reopen with the last path predicted.
  useEffect(() => {
    if (autoRan.current || !request || !txPosition.trim() || !rxPosition.trim()) return;
    autoRan.current = true;
    predict();
  }, [request, txPosition, rxPosition, predict]);

  // Keyboard: Ctrl+Enter predicts, Alt+1..9 switch views, [ and ] step the hour.
  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if ((event.ctrlKey || event.metaKey) && event.key === "Enter") {
        event.preventDefault();
        predict();
        return;
      }
      if (event.altKey && /^[0-9]$/.test(event.key)) {
        const index = event.key === "0" ? 9 : Number(event.key) - 1;
        if (VIEWS[index]) {
          event.preventDefault();
          setView(VIEWS[index].id);
        }
        return;
      }
      const target = event.target instanceof Element ? event.target : null;
      if (target?.closest("input, select, textarea, [contenteditable]")) return;
      if (event.key === "[") setClockHour((h) => (h + 23) % 24);
      if (event.key === "]") setClockHour((h) => (h + 1) % 24);
      if (event.key === "n") setClockHour(new Date().getUTCHours());
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [predict]);

  if (!options || !txStation || !rxStation) {
    return <div className="empty" style={{ padding: 32 }}>Starting…</div>;
  }

  const overview = run.kind === "done" ? run.result : null;
  const detail = overview && (longPath ? overview.long : overview.short);
  const other = overview && (longPath ? overview.short : overview.long);
  const hours = detail?.prediction.run.prediction.hours ?? [];
  const hourIndex = Math.max(0, hours.findIndex((h) => clockOf(h.utcHour) === clockHour));
  const selectHour = (index: number) => setClockHour(clockOf(hours[index].utcHour));
  const modeLabel = options.modes.find((m) => m.value === mode)?.label ?? mode;
  const changed = run.kind === "done" && run.key !== requestKey;
  const staleData =
    conditions !== null &&
    (conditions.ssnTable.stale || conditions.products.some((p) => p.stale || !p.stored));
  const clock = live.listener?.state === "listening" ? live.listener.tracker?.clock : null;
  const current = VIEWS.find((v) => v.id === view) ?? VIEWS[0];
  const profiles = [...options.presets, ...userData.stations];
  const ssnValue = ssn.trim() === "" ? null : Number(ssn);
  const updateState = updater.state;
  const updateVersion =
    updateState.kind === "available"
      ? updateState.update.version
      : updateState.kind === "downloading" || updateState.kind === "installing"
        ? updateState.version
        : null;
  const noticeText = updateNoticeText(updateState);
  const showNotice = noticeText !== null && (updateVersion === null || updateVersion !== laterFor);
  const openUpdates = () => {
    setView("setup");
    setTimeout(() => document.getElementById("updates")?.scrollIntoView({ block: "center" }), 50);
  };

  const stationSelect = (station: StationProfile, set: (s: StationProfile) => void) => (
    <select
      value={profiles.some((p) => p.name === station.name) ? station.name : ""}
      onChange={(e) => {
        const picked = profiles.find((p) => p.name === e.target.value);
        if (picked) set(picked);
      }}
    >
      <option value="" disabled>
        {station.name} (edited)
      </option>
      {profiles.map((p) => (
        <option key={p.name}>{p.name}</option>
      ))}
    </select>
  );

  return (
    <div className="shell">
      <StatusBar
        version={version}
        detail={detail}
        longPath={longPath}
        conditions={conditions}
        live={live}
        nowS={nowS}
        onNavigate={setView}
        onEditPath={() => fromRef.current?.focus()}
        update={updateState.kind === "available" ? updateState.update.version : null}
        updateAttention={showNotice}
        onUpdate={openUpdates}
      >
        {detail && <BestNow detail={detail} nowClock={nowClock} onOpen={() => setView("compare")} />}
      </StatusBar>

      <div className="body">
        <nav className="nav" aria-label="Views">
          {NAV.map((group) => (
            <div className="nav-group" key={group.group}>
              <h2>{group.group}</h2>
              {group.items.map((item) => {
                const index = VIEWS.indexOf(item);
                return (
                  <button
                    key={item.id}
                    type="button"
                    aria-current={view === item.id ? "page" : undefined}
                    title={`Alt+${index === 9 ? 0 : index + 1}`}
                    onClick={() => setView(item.id)}
                  >
                    {item.label}
                    {item.id === "conditions" && staleData && (
                      <span className="flag" title="Some data is old or missing">
                        ◐
                      </span>
                    )}
                    {item.needsPath && !detail && <span className="needs">path</span>}
                  </button>
                );
              })}
            </div>
          ))}
        </nav>

        <div className="main">
          <form
            className="pathbar"
            onSubmit={(e) => {
              e.preventDefault();
              predict();
            }}
          >
            <label className="loc pb-from">
              From
              <input
                ref={fromRef}
                name="from"
                list="saved-locations"
                placeholder="EM73tr"
                value={txPosition}
                onChange={(e) => setTxPosition(e.target.value)}
                aria-label="From: locator or latitude, longitude"
              />
            </label>
            <button
              type="button"
              className="quiet swap"
              title="Swap From and To"
              aria-label="Swap From and To"
              onClick={() => {
                setTxPosition(rxPosition);
                setRxPosition(txPosition);
              }}
            >
              ⇄
            </button>
            <label className="loc pb-to">
              To
              <input
                name="to"
                list="saved-locations"
                placeholder="IO91wm"
                value={rxPosition}
                onChange={(e) => setRxPosition(e.target.value)}
                aria-label="To: locator or latitude, longitude"
              />
            </label>
            <datalist id="saved-locations">
              {userData.locations.map((l) => (
                <option key={l.name} value={l.position}>
                  {l.name}
                </option>
              ))}
            </datalist>
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
            <div className="pf">
              <span>Path</span>
              <div className="segmented" role="group" aria-label="Short or long path">
                <button type="button" aria-pressed={!longPath} onClick={() => setLongPath(false)}>
                  Short
                </button>
                <button type="button" aria-pressed={longPath} onClick={() => setLongPath(true)}>
                  Long
                </button>
              </div>
            </div>
            <span className="pb-break" aria-hidden="true" />
            <div className="pf pb-hour">
              <span>Hour shown</span>
              <div className="hourstep">
                <button type="button" aria-label="Previous hour" title="Previous hour ( [ )" onClick={() => setClockHour((clockHour + 23) % 24)}>
                  ‹
                </button>
                <output aria-live="polite">
                  <span className="num">{String(clockHour).padStart(2, "0")} UTC</span>
                  <span className="hint">
                    {localHour(clockHour, zone)} {zone.name}
                  </span>
                </output>
                <button type="button" aria-label="Next hour" title="Next hour ( ] )" onClick={() => setClockHour((clockHour + 1) % 24)}>
                  ›
                </button>
                <button
                  type="button"
                  title="The current hour ( n )"
                  aria-pressed={clockHour === nowClock}
                  onClick={() => setClockHour(nowClock)}
                >
                  Now
                </button>
              </div>
            </div>
            <div className="actions">
              {run.kind === "failed" && <span className="error">Prediction failed</span>}
              <button type="button" className="quiet" aria-expanded={moreOpen} onClick={() => setMoreOpen(!moreOpen)}>
                {moreOpen ? "Fewer settings" : "More settings"}
              </button>
              <button
                className={changed ? "primary stale" : "primary"}
                type="submit"
                disabled={run.kind === "running" || !txPosition.trim() || !rxPosition.trim()}
                title={changed ? "Settings changed since the last prediction (Ctrl+Enter)" : "Ctrl+Enter"}
              >
                {run.kind === "running" ? "Predicting…" : changed ? "Update" : "Predict"}
              </button>
            </div>
            {moreOpen && (
              <div className="pathbar-more">
                <label>
                  Year
                  <input type="number" min={1990} max={2100} value={year} onChange={(e) => setYear(Number(e.target.value))} />
                </label>
                <label>
                  Month
                  <input type="number" min={1} max={12} value={month} onChange={(e) => setMonth(Number(e.target.value))} />
                </label>
                <label title="The share of days a path must work to count as reliable">
                  Required reliability %
                  <input type="number" min={10} max={99} value={reliability} onChange={(e) => setReliability(Number(e.target.value))} />
                </label>
                <label title="Leave blank to use the NOAA smoothed sunspot table">
                  Sunspot number
                  <input
                    type="number"
                    min={0}
                    max={300}
                    step="any"
                    placeholder="table"
                    value={ssn}
                    onChange={(e) => setSsn(e.target.value)}
                  />
                </label>
                <label>
                  My station
                  {stationSelect(txStation, setTxStation)}
                </label>
                <label>
                  Other station
                  {stationSelect(rxStation, setRxStation)}
                </label>
                <button type="button" className="quiet" onClick={() => setView("setup")}>
                  Edit stations and locations
                </button>
              </div>
            )}
          </form>

          {conditions?.storm && (
            <div className="alertline" role="alert">
              <strong>Geomagnetic storm</strong>
              <span>{conditions.storm}</span>
            </div>
          )}
          {clock && (clock.level === "warn" || clock.level === "alarm") && (
            <div className="alertline caution" role="alert">
              <strong>Clock</strong>
              <span>
                This computer's clock looks off
                {clock.medianDtS !== null && ` by about ${Math.abs(clock.medianDtS).toFixed(1)} s`}, from{" "}
                {clock.samples} decodes. FT8 needs it within about a second, FT4 and FT2 tighter still.
                Synchronise it, or decoding will suffer.
              </span>
            </div>
          )}
          {showNotice && (
            <div className="alertline notice" role="status">
              <strong>Update</strong>
              <span>{noticeText}</span>
              {updateState.kind === "available" && (
                <span className="alert-actions">
                  <button type="button" className="primary" onClick={() => updater.install(updateState.update)}>
                    Install and restart
                  </button>
                  <button type="button" className="quiet" onClick={() => setLaterFor(updateState.update.version)}>
                    Later
                  </button>
                </span>
              )}
              {updateState.kind === "failed" && (
                <span className="alert-actions">
                  <button type="button" className="quiet" onClick={openUpdates}>
                    Open Updates
                  </button>
                </span>
              )}
            </div>
          )}
          {loadError && (
            <div className="alertline caution">
              <strong>Storage</strong>
              <span>{loadError}</span>
            </div>
          )}

          <main className="view" id="view">
            {current.needsPath && !detail && (
              <div className="empty">
                <h2>{current.label} needs a predicted path</h2>
                {run.kind === "running" && <p>Predicting…</p>}
                {run.kind === "failed" && <p className="error">The prediction failed: {run.error}</p>}
                {run.kind !== "running" && (
                  <p>
                    Enter your position in From and the other end in To, as a locator such as EM73tr
                    or as latitude, longitude, then press Predict or Ctrl+Enter.
                  </p>
                )}
              </div>
            )}

            {view === "field" && detail && (
              <FieldPanel
                detail={detail}
                hourIndex={hourIndex}
                modeLabel={modeLabel}
                longPath={longPath}
                month={month}
                zone={zone}
                conditions={conditions}
                nowClock={nowClock}
              />
            )}

            {view === "bands" && detail && other && (
              <>
                <BestBands
                  detail={detail}
                  other={other}
                  otherName={longPath ? "short" : "long"}
                  hourIndex={hourIndex}
                  zone={zone}
                  nowClock={nowClock}
                  modeLabel={modeLabel}
                  longPath={longPath}
                />
                <PowerTable detail={detail} hourIndex={hourIndex} zone={zone} />
              </>
            )}

            {view === "day" && detail && (
              <BandLadder
                detail={detail}
                hourIndex={hourIndex}
                onSelectHour={selectHour}
                zone={zone}
                nowClock={nowClock}
                modeLabel={modeLabel}
              />
            )}

            {view === "compare" && detail && (
              <ComparePanel detail={detail} hourIndex={hourIndex} modeLabel={modeLabel} zone={zone} />
            )}

            {view === "map" && (
              <MapPanel
                txPosition={txPosition}
                rxPosition={rxPosition}
                longPath={longPath}
                year={year}
                month={month}
                ssn={ssnValue}
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

            {view === "heard" && (
              <HeardPanel
                logFiles={userData.logFiles}
                onLogFilesChange={(logFiles) => saveUserData({ ...userData, logFiles })}
                defaultRxPosition={txPosition}
              />
            )}

            {view === "plan" && (
              <PlanPanel
                txPosition={txPosition}
                rxPosition={rxPosition}
                year={year}
                month={month}
                ssn={ssnValue}
                txStation={txStation}
                rxStation={rxStation}
                clockHour={clockHour}
                zone={zone}
              />
            )}

            {view === "radio" && <RadioPanel />}

            {view === "history" && <HistoryPanel rxPosition={txPosition} noiseDb={txStation.noiseDb} />}

            {view === "conditions" &&
              (conditions ? (
                <ConditionsPanel conditions={conditions} onUpdate={setConditions} ssn={detail?.prediction.ssn ?? null} />
              ) : (
                <p className="note">Conditions could not be loaded.</p>
              ))}

            {view === "setup" && (
              <SetupPanel
                options={options}
                userData={userData}
                onUserData={saveUserData}
                txStation={txStation}
                rxStation={rxStation}
                onTxStation={setTxStation}
                onRxStation={setRxStation}
                onUseLocation={(position, end) => (end === "from" ? setTxPosition(position) : setRxPosition(position))}
                theme={theme}
                onTheme={setTheme}
                version={version}
                updater={updater}
              />
            )}

            {view === "engine" && detail && <EnginePanel detail={detail} />}
          </main>
        </div>
      </div>
    </div>
  );
}

export default App;
