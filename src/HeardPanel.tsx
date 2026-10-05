import { ChangeEvent, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  BandActivity,
  ImportSummary,
  ListenerConfig,
  ListenerStatus,
  Observation,
} from "./types";

const POLL_MS = 3000;
const RECENT_LIMIT = 100;
const WINDOWS = [
  { minutes: 15, label: "15 minutes" },
  { minutes: 60, label: "hour" },
  { minutes: 360, label: "6 hours" },
  { minutes: 1440, label: "24 hours" },
];

const CLOCK = {
  unknown: "not enough decodes yet to check the clock",
  ok: "clock looks right",
  warn: "clock may be off; FT8 needs it within about a second",
  alarm: "clock is off by a second or more; decoding will suffer until it is corrected",
};

function clock(unixSeconds: number): string {
  return new Date(unixSeconds * 1000).toISOString().slice(11, 19);
}

const COMPASS = ["N", "NE", "E", "SE", "S", "SW", "W", "NW"];

/** Cell shading for a count against the largest in its row: one hue, stronger with more. */
function countShade(count: number, largest: number): string {
  return `rgba(57, 135, 229, ${(largest > 0 ? (count / largest) * 0.7 : 0).toFixed(2)})`;
}

const km = (value: number | null) => (value === null ? "—" : `${value.toFixed(0)} km`);
const db = (value: number | null) => (value === null ? "—" : `${value.toFixed(0)} dB`);

function duration(seconds: number): string {
  if (seconds < 90) return `${seconds} s`;
  if (seconds < 5400) return `${Math.round(seconds / 60)} min`;
  return `${(seconds / 3600).toFixed(1)} h`;
}

/** What WSJT-X is decoding right now, as received over the network. */
export function HeardPanel() {
  const [status, setStatus] = useState<ListenerStatus | null>(null);
  const [draft, setDraft] = useState<ListenerConfig | null>(null);
  const [minutes, setMinutes] = useState(60);
  const [activity, setActivity] = useState<BandActivity[]>([]);
  const [recent, setRecent] = useState<Observation[]>([]);
  const [error, setError] = useState("");
  const [rxGrid, setRxGrid] = useState("");
  const [imported, setImported] = useState("");

  useEffect(() => {
    let current = true;
    async function poll() {
      try {
        const [nextStatus, nextActivity, nextRecent] = await Promise.all([
          invoke<ListenerStatus>("listener_status"),
          invoke<BandActivity[]>("band_activity", { minutes }),
          invoke<Observation[]>("recent_observations", { limit: RECENT_LIMIT }),
        ]);
        if (!current) return;
        setStatus(nextStatus);
        setDraft((existing) => existing ?? nextStatus.config);
        setActivity(nextActivity);
        setRecent(nextRecent);
        setError("");
      } catch (e) {
        if (current) setError(String(e));
      }
    }
    poll();
    const timer = setInterval(poll, POLL_MS);
    return () => {
      current = false;
      clearInterval(timer);
    };
  }, [minutes]);

  async function apply() {
    if (!draft) return;
    try {
      setStatus(await invoke<ListenerStatus>("set_listener_config", { config: draft }));
      setError("");
    } catch (e) {
      setError(String(e));
    }
  }

  async function importLog(event: ChangeEvent<HTMLInputElement>) {
    const file = event.target.files?.[0];
    event.target.value = "";
    if (!file) return;
    setImported("Importing…");
    try {
      const summary = await invoke<ImportSummary>("import_all_txt", {
        text: await file.text(),
        rxGrid: rxGrid.trim() || null,
      });
      setImported(
        `${file.name}: ${summary.stored} decodes imported, ${summary.alreadyStored} already stored, ` +
          `${summary.transmissions} own transmissions skipped, ${summary.notUnderstood} lines not understood.`,
      );
    } catch (e) {
      setImported(`${file.name}: ${e}`);
    }
  }

  if (!status || !draft) return <p className="note">{error || "Loading…"}</p>;
  const tracker = status.tracker;

  return (
    <section className="heard">
      {error && <p className="error">{error}</p>}

      <div className="controls">
        <label className="inline">
          <input
            type="checkbox"
            checked={draft.enabled}
            onChange={(e) => setDraft({ ...draft, enabled: e.target.checked })}
          />
          Listen for WSJT-X
        </label>
        <label className="inline">
          Address
          <input
            className="short"
            value={draft.address}
            onChange={(e) => setDraft({ ...draft, address: e.target.value })}
          />
        </label>
        <label className="inline">
          Port
          <input
            className="short"
            type="number"
            min={1}
            max={65535}
            value={draft.port}
            onChange={(e) => setDraft({ ...draft, port: Number(e.target.value) })}
          />
        </label>
        <button type="button" onClick={apply}>
          Apply
        </button>
      </div>

      <p className={status.state === "failed" ? "error" : undefined}>
        {status.state === "failed" ? `Cannot listen: ${status.detail}` : status.detail}
        {status.state === "listening" &&
          ` ${status.datagrams} messages received` +
            (status.notUnderstood > 0 ? `, ${status.notUnderstood} not from WSJT-X.` : ".")}
      </p>
      {status.lastError && <p className="error">Last problem: {status.lastError}</p>}

      {status.state === "listening" && tracker && (
        <>
          {tracker.decoders.length === 0 && (
            <p className="note">
              No decoder heard yet. Check that WSJT-X is running and set up as described below.
            </p>
          )}
          {tracker.decoders.map((d) => (
            <p key={d.id}>
              <strong>
                {d.id} {d.version}
              </strong>
              {d.dialHz !== null && ` · ${(d.dialHz / 1e6).toFixed(3)} MHz (${d.band}) ${d.mode}`}
              {d.deCall && ` · ${d.deCall} ${d.deGrid ?? ""}`}
              {d.transmitting && " · transmitting"} · last heard {d.secondsSinceHeard} s ago
            </p>
          ))}
          <p className={`clock clock-${tracker.clock.level}`}>
            Clock check:{" "}
            {tracker.clock.medianDtS !== null &&
              `median time offset ${tracker.clock.medianDtS > 0 ? "+" : ""}${tracker.clock.medianDtS.toFixed(1)} s ` +
                `over ${tracker.clock.samples} decodes — `}
            {CLOCK[tracker.clock.level]}.
          </p>
        </>
      )}

      <details>
        <summary>Setting up WSJT-X</summary>
        <ol>
          <li>
            In WSJT-X open File → Settings → Reporting. Under UDP Server set the address to{" "}
            <code>224.0.0.1</code> and the port to <code>2237</code>, and tick the loopback
            interface under Outgoing interfaces.
          </li>
          <li>Enter the same address and port above, tick Listen for WSJT-X and press Apply.</li>
          <li>
            Set GridTracker, JTAlert or other programs to the same multicast address. Every program
            that joins the group receives everything.
          </li>
        </ol>
        <p className="note">
          An address starting 224 to 239 is a multicast group, which programs share. An ordinary
          address such as 127.0.0.1 can be received by one program only: use it only if nothing
          else listens on that port, or point this app at a port another program forwards to
          (GridTracker forwards to 2238). This app only listens. It never sends anything to WSJT-X
          and never touches the radio.
        </p>
      </details>

      <h3>
        Heard in the last{" "}
        <select value={minutes} onChange={(e) => setMinutes(Number(e.target.value))}>
          {WINDOWS.map((w) => (
            <option key={w.minutes} value={w.minutes}>
              {w.label}
            </option>
          ))}
        </select>
      </h3>
      {activity.length === 0 ? (
        <p className="note">Nothing listened to in this period.</p>
      ) : (
        <div className="scroll-x">
          <table className="results compact">
            <thead>
              <tr>
                <th rowSpan={2}>Band</th>
                <th rowSpan={2}>Listened</th>
                <th colSpan={2}>Decodes</th>
                <th colSpan={2}>Stations</th>
                <th colSpan={2}>SNR</th>
                <th colSpan={3}>Distance</th>
                <th colSpan={COMPASS.length}>Stations by direction</th>
              </tr>
              <tr>
                <th>Total</th>
                <th>Per period</th>
                <th>Callsigns</th>
                <th>Locators</th>
                <th>Median</th>
                <th>90% under</th>
                <th>Median</th>
                <th>Farthest</th>
                <th>Over 3000 km</th>
                {COMPASS.map((point) => (
                  <th key={point}>{point}</th>
                ))}
              </tr>
            </thead>
            <tbody>
              {activity.map((a) => {
                const largest = Math.max(...a.sectors);
                return (
                  <tr key={a.band}>
                    <th>{a.band}</th>
                    <td>{duration(a.listenedSeconds)}</td>
                    <td>{a.decodes}</td>
                    <td>{a.decodesPerPeriod === null ? "—" : a.decodesPerPeriod.toFixed(1)}</td>
                    <td>
                      {a.uniqueCallsigns}
                      {a.previousPeriods > 0 && (
                        <span
                          className="hint"
                          title={`In the same span before this one: ${a.previousUniqueCallsigns} callsigns in ${a.previousPeriods.toFixed(0)} periods of listening`}
                        >
                          {" "}
                          (was {a.previousUniqueCallsigns})
                        </span>
                      )}
                    </td>
                    <td>{a.uniqueGrids}</td>
                    <td>{db(a.medianSnrDb)}</td>
                    <td>{db(a.p90SnrDb)}</td>
                    <td>{km(a.medianDistanceKm)}</td>
                    <td>{km(a.maxDistanceKm)}</td>
                    <td>{a.longDistanceStations}</td>
                    {a.sectors.map((count, i) => (
                      <td key={COMPASS[i]} style={{ background: countShade(count, largest) }}>
                        {count}
                      </td>
                    ))}
                  </tr>
                );
              })}
            </tbody>
          </table>
        </div>
      )}
      <p className="note">
        Signal figures are over decodes; distance and direction figures count each station once.
        "Per period" is decodes per transmit period listened. "Was" compares callsigns with the
        same span just before. This is observed activity, not a measurement of the ionosphere. It depends on who is on
        the air, their power and antennas, and local noise. No signals heard does not mean a band
        is closed, and hearing a station does not mean it can hear you.
      </p>

      <h3>Latest decodes</h3>
      {recent.length === 0 ? (
        <p className="note">None stored yet.</p>
      ) : (
        <div className="scroll tall">
          <table className="results compact">
            <thead>
              <tr>
                <th>UTC</th>
                <th>Band</th>
                <th>SNR</th>
                <th>DT</th>
                <th>Message</th>
                <th>Locator</th>
                <th>km</th>
                <th>Bearing</th>
              </tr>
            </thead>
            <tbody>
              {recent.map((o, i) => (
                <tr key={i} className={o.settling ? "muted" : undefined}>
                  <th>{clock(o.timeUtc)}</th>
                  <td>{o.band}</td>
                  <td>{o.snrDb}</td>
                  <td>{o.dtS.toFixed(1)}</td>
                  <td className="left mono">{o.message}</td>
                  <td>
                    {o.grid ?? "—"}
                    {o.gridSource === "remembered" && "*"}
                  </td>
                  <td>{o.distanceKm === null ? "—" : o.distanceKm.toFixed(0)}</td>
                  <td>{o.bearingDeg === null ? "—" : `${o.bearingDeg.toFixed(0)}°`}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
      <p className="note">
        * The station sent no locator in this message; the one from its earlier message is used.
        Greyed rows were decoded while the receiver was changing frequency and are left out of the
        counts.
      </p>

      <h3>Import a WSJT-X log</h3>
      <div className="controls">
        <label className="inline">
          Receiver locator
          <input
            className="short"
            placeholder="EM73"
            value={rxGrid}
            onChange={(e) => setRxGrid(e.target.value)}
          />
        </label>
        <label className="file">
          ALL.TXT
          <input type="file" accept=".txt,.TXT" onChange={importLog} />
        </label>
      </div>
      <p className="note">
        The log does not record where the receiver was. Give the locator to get distances and
        bearings. {imported}
      </p>
    </section>
  );
}
