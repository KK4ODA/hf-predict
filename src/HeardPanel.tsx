import { localClock } from "./localtime";
import { useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { LogFiles } from "./LogFiles";
import { Health, Pill } from "./ui";
import { BandActivity, ListenerConfig, ListenerStatus, LogFile, Observation } from "./types";

const POLL_MS = 3000;
const RECENT_LIMIT = 200;
const WINDOWS = [
  { minutes: 15, label: "15 minutes" },
  { minutes: 60, label: "hour" },
  { minutes: 360, label: "6 hours" },
  { minutes: 1440, label: "24 hours" },
];

const CLOCK = {
  unknown: "Not enough decodes yet to check the clock.",
  ok: "The clock looks right.",
  warn: "The clock may be off; FT8 needs it within about a second.",
  alarm: "The clock is off by a second or more; decoding will suffer until it is corrected.",
};

const COMPASS = ["N", "NE", "E", "SE", "S", "SW", "W", "NW"];

function utcClock(unixSeconds: number): string {
  return new Date(unixSeconds * 1000).toISOString().slice(11, 19);
}

const km = (value: number | null) => (value === null ? "—" : value.toFixed(0));
const db = (value: number | null) => (value === null ? "—" : value.toFixed(0));

function duration(seconds: number): string {
  if (seconds < 90) return `${seconds} s`;
  if (seconds < 5400) return `${Math.round(seconds / 60)} min`;
  return `${(seconds / 3600).toFixed(1)} h`;
}

/** Stations by compass sector as a small rose, north up. */
function Rose({ sectors }: { sectors: number[] }) {
  const largest = Math.max(1, ...sectors);
  const r = 15;
  const wedge = (i: number, length: number) => {
    const a0 = ((i * 45 - 22.5 - 90) * Math.PI) / 180;
    const a1 = ((i * 45 + 22.5 - 90) * Math.PI) / 180;
    const p = (a: number) => `${(18 + length * Math.cos(a)).toFixed(1)},${(18 + length * Math.sin(a)).toFixed(1)}`;
    return `M18,18 L${p(a0)} A${length},${length} 0 0 1 ${p(a1)} Z`;
  };
  const label = sectors.map((n, i) => `${COMPASS[i]} ${n}`).join(", ");
  return (
    <svg className="rose" width={36} height={36} viewBox="0 0 36 36" role="img" aria-label={`Stations by direction: ${label}`}>
      <title>{label}</title>
      <circle cx={18} cy={18} r={r} fill="none" stroke="var(--rule)" />
      {sectors.map((n, i) =>
        n > 0 ? <path key={i} d={wedge(i, Math.max(3, Math.sqrt(n / largest) * r))} fill="var(--meas)" /> : null,
      )}
      <line x1={18} y1={2} x2={18} y2={6} stroke="var(--ink-3)" />
    </svg>
  );
}

type SortKey = "time" | "snr" | "distance";

type Props = {
  logFiles: LogFile[];
  onLogFilesChange: (files: LogFile[]) => void;
  defaultRxPosition: string;
};

/** What this station's receiver decoded, live from WSJT-X and from its logs. */
export function HeardPanel({ logFiles, onLogFilesChange, defaultRxPosition }: Props) {
  const [status, setStatus] = useState<ListenerStatus | null>(null);
  const [draft, setDraft] = useState<ListenerConfig | null>(null);
  const [minutes, setMinutes] = useState(60);
  const [activity, setActivity] = useState<BandActivity[]>([]);
  const [recent, setRecent] = useState<Observation[]>([]);
  const [error, setError] = useState("");
  const [bandFilter, setBandFilter] = useState("all");
  const [search, setSearch] = useState("");
  const [sort, setSort] = useState<{ key: SortKey; descending: boolean }>({ key: "time", descending: true });

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

  const bands = useMemo(() => [...new Set(recent.map((o) => o.band))].sort((a, b) => parseFloat(b) - parseFloat(a)), [recent]);
  const shown = useMemo(() => {
    const needle = search.trim().toUpperCase();
    const value = (o: Observation) =>
      sort.key === "time" ? o.timeUtc : sort.key === "snr" ? o.snrDb : (o.distanceKm ?? -1);
    return recent
      .filter((o) => bandFilter === "all" || o.band === bandFilter)
      .filter((o) => !needle || o.message.toUpperCase().includes(needle) || (o.grid ?? "").toUpperCase().includes(needle))
      .sort((a, b) => (sort.descending ? value(b) - value(a) : value(a) - value(b)));
  }, [recent, bandFilter, search, sort]);

  if (!status || !draft) return <p className="note">{error || "Loading…"}</p>;
  const tracker = status.tracker;
  const listening: Health =
    status.state === "failed" ? "alert" : status.state === "off" ? "off" : tracker?.decoders.length ? "ok" : "warn";
  const sortHeader = (key: SortKey, label: string) => (
    <th
      className="sortable"
      aria-sort={sort.key === key ? (sort.descending ? "descending" : "ascending") : undefined}
      onClick={() => setSort({ key, descending: sort.key === key ? !sort.descending : true })}
    >
      {label}
    </th>
  );

  return (
    <section className="heard">
      <div className="view-head">
        <h2>Heard</h2>
        <span className="hint">What your receiver decoded. This app only listens; it never sends anything to WSJT-X.</span>
      </div>
      {error && <p className="error">{error}</p>}

      <div className="panel">
        <div className="listener">
          <Pill state={listening}>
            {status.state === "failed"
              ? "Cannot listen"
              : status.state === "off"
                ? "Not listening"
                : tracker?.decoders.length
                  ? "Receiving"
                  : "Waiting for WSJT-X"}
          </Pill>
          <label className="inline">
            <input type="checkbox" checked={draft.enabled} onChange={(e) => setDraft({ ...draft, enabled: e.target.checked })} />
            Listen for WSJT-X
          </label>
          <label className="inline">
            Address
            <input className="short" value={draft.address} onChange={(e) => setDraft({ ...draft, address: e.target.value })} />
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
          <span className="hint">
            {status.state === "listening"
              ? `${status.datagrams} messages${status.notUnderstood > 0 ? `, ${status.notUnderstood} not from WSJT-X` : ""}`
              : status.detail}
          </span>
        </div>
        {status.lastError && <p className="error">Last problem: {status.lastError}</p>}
        {status.state === "listening" && tracker && (
          <>
            {tracker.decoders.map((d) => (
              <div key={d.id} className="decoder">
                <strong>
                  {d.id} {d.version}
                </strong>
                {d.dialHz !== null && (
                  <span>
                    <span className="figure">{(d.dialHz / 1e6).toFixed(3)}</span> MHz {d.band} {d.mode}
                  </span>
                )}
                {d.deCall && (
                  <span>
                    {d.deCall} {d.deGrid ?? ""}
                  </span>
                )}
                {d.transmitting && <Pill state="alert">Transmitting</Pill>}
                <span className="hint">last message {d.secondsSinceHeard} s ago</span>
              </div>
            ))}
            {tracker.decoders.length === 0 && (
              <p className="note">No decoder heard yet. Check that WSJT-X is running and set up as below.</p>
            )}
            <p className={`note clock-${tracker.clock.level}`} style={{ marginBottom: 0 }}>
              Clock check:{" "}
              {tracker.clock.medianDtS !== null &&
                `median time offset ${tracker.clock.medianDtS > 0 ? "+" : ""}${tracker.clock.medianDtS.toFixed(1)} s over ${tracker.clock.samples} decodes. `}
              {CLOCK[tracker.clock.level]}
            </p>
          </>
        )}
        <details>
          <summary>Setting up WSJT-X</summary>
          <ol style={{ maxWidth: "80ch", paddingLeft: 18 }}>
            <li>
              In WSJT-X open File, Settings, Reporting. Under UDP Server set the address to{" "}
              <code>224.0.0.1</code> and the port to <code>2237</code>, and tick the loopback interface
              under Outgoing interfaces.
            </li>
            <li>Enter the same address and port above, tick Listen for WSJT-X and press Apply.</li>
            <li>Set GridTracker, JTAlert and other programs to the same address. Every program in the group receives everything.</li>
          </ol>
          <p className="note">
            An address from 224 to 239 is a multicast group, which programs share. An ordinary address
            such as 127.0.0.1 can be received by one program only: use it only if nothing else listens
            on that port, or point this app at a port another program forwards to (GridTracker
            forwards to 2238).
          </p>
        </details>
      </div>

      <div className="section-title" style={{ marginTop: 18 }}>
        <h3>Activity by band</h3>
        <label className="inline hint">
          over the last
          <select value={minutes} onChange={(e) => setMinutes(Number(e.target.value))}>
            {WINDOWS.map((w) => (
              <option key={w.minutes} value={w.minutes}>
                {w.label}
              </option>
            ))}
          </select>
        </label>
      </div>
      {activity.length === 0 ? (
        <p className="note">Nothing was listened to in this period.</p>
      ) : (
        <div className="scroll-x">
          <table className="data">
            <thead>
              <tr>
                <th className="left">Band</th>
                <th>Listened</th>
                <th>Decodes</th>
                <th>Per period</th>
                <th>Callsigns</th>
                <th>Locators</th>
                <th title="Median and 90th percentile SNR, dB">SNR median, p90</th>
                <th title="Median and farthest distance, km">Distance median, max</th>
                <th>Over 3000 km</th>
                <th>Direction</th>
              </tr>
            </thead>
            <tbody>
              {activity.map((a) => (
                <tr key={a.band}>
                  <th className="band">{a.band}</th>
                  <td>{duration(a.listenedSeconds)}</td>
                  <td className="num">{a.decodes}</td>
                  <td className="num">{a.decodesPerPeriod === null ? "—" : a.decodesPerPeriod.toFixed(1)}</td>
                  <td>
                    <span className="num">{a.uniqueCallsigns}</span>
                    {a.previousPeriods > 0 && (
                      <span
                        className="hint"
                        title={`In the same span before this one: ${a.previousUniqueCallsigns} callsigns over ${a.previousPeriods.toFixed(0)} periods of listening`}
                      >
                        {" "}
                        was {a.previousUniqueCallsigns}
                      </span>
                    )}
                  </td>
                  <td className="num">{a.uniqueGrids}</td>
                  <td className="num">
                    {db(a.medianSnrDb)} <span className="hint">{db(a.p90SnrDb)}</span>
                  </td>
                  <td className="num">
                    {km(a.medianDistanceKm)} <span className="hint">{km(a.maxDistanceKm)}</span>
                  </td>
                  <td className="num">{a.longDistanceStations}</td>
                  <td>
                    <Rose sectors={a.sectors} />
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
      <p className="note">
        Signal figures are over decodes; distance and direction count each station once, and the rose
        is drawn north up with area proportional to stations. "Per period" is decodes per transmit
        period listened; "was" compares callsigns with the same span just before. This is activity, not
        a measurement of the ionosphere: it depends on who is on the air. Nothing heard does not mean a
        band is closed, and hearing a station does not mean it can hear you.
      </p>

      <div className="section-title" style={{ marginTop: 18 }}>
        <h3>Latest decodes</h3>
        <select value={bandFilter} onChange={(e) => setBandFilter(e.target.value)} aria-label="Band">
          <option value="all">All bands</option>
          {bands.map((b) => (
            <option key={b} value={b}>
              {b}
            </option>
          ))}
        </select>
        <input
          type="search"
          placeholder="Find a callsign or locator"
          value={search}
          onChange={(e) => setSearch(e.target.value)}
          style={{ width: 210 }}
        />
        <span className="hint">
          {shown.length} of {recent.length}
        </span>
      </div>
      {recent.length === 0 ? (
        <p className="note">None stored yet. Turn on listening above, or add a WSJT-X log below.</p>
      ) : (
        <div className="scroll tall">
          <table className="data decodes">
            <thead>
              <tr>
                {sortHeader("time", "UTC")}
                <th>Local</th>
                <th className="left">Band</th>
                {sortHeader("snr", "SNR")}
                <th>DT</th>
                <th className="left">Message</th>
                <th className="left">Locator</th>
                {sortHeader("distance", "km")}
                <th>Bearing</th>
                <th className="left">From</th>
              </tr>
            </thead>
            <tbody>
              {shown.map((o) => (
                <tr key={`${o.timeUtc} ${o.dialHz} ${o.dfHz} ${o.message}`} className={o.settling ? "muted" : undefined}>
                  <td className="num">{utcClock(o.timeUtc)}</td>
                  <td className="num hint">{localClock(o.timeUtc)}</td>
                  <td className="left">{o.band}</td>
                  <td className="num">{o.snrDb}</td>
                  <td className="num">{o.dtS.toFixed(1)}</td>
                  <td className="msg">{o.message}</td>
                  <td className="left">
                    {o.grid ?? "—"}
                    {o.gridSource === "remembered" && <span title="Remembered from an earlier message">*</span>}
                  </td>
                  <td className="num">{km(o.distanceKm)}</td>
                  <td className="num">{o.bearingDeg === null ? "—" : `${o.bearingDeg.toFixed(0)}°`}</td>
                  <td className="left prov">{o.provider === "wsjtx-udp" ? "live" : o.provider === "all.txt" ? "log" : o.provider}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
      <p className="note">
        * The station sent no locator in this message; the one from its earlier message is used. Grey
        rows were decoded while the receiver was changing frequency and are left out of the counts.
        "Live" decodes came over the network from WSJT-X; "log" decodes were read from an ALL.TXT file.
      </p>

      <LogFiles files={logFiles} onChange={onLogFilesChange} defaultRxPosition={defaultRxPosition} />
    </section>
  );
}
