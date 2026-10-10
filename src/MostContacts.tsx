import { useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { hourBoth, localHour, Zone } from "./localtime";
import { Meter, RelScale, relStyle, Ring } from "./ui";
import { BandActivity, ContactsReport, HearingYourArea, Mode, StationProfile } from "./types";

type Props = {
  txPosition: string;
  year: number;
  month: number;
  ssn: number | null;
  txStation: StationProfile;
  rxStation: StationProfile;
  mode: Mode;
  modeLabel: string;
  reliability: number;
  clockHour: number;
  nowClock: number;
  zone: Zone;
  onSelectHour: (clockHour: number) => void;
};

type Progress = { done: number; total: number };

const SPANS = [
  { days: 0, label: "all of it" },
  { days: 365, label: "the last year" },
  { days: 90, label: "the last 90 days" },
  { days: 30, label: "the last 30 days" },
];
const DISTANCES = ["Under 1,000 km", "1,000 to 3,000", "3,000 to 8,000", "Over 8,000"];
const NOW_MINUTES = 60;
const POLL_MS = 15000;
const ALL_HOURS = Array.from({ length: 24 }, (_, h) => h);

const round = (value: number) => Math.round(value).toLocaleString();

/** What was heard on each band in the last hour, both directions. */
function useHeardNow(receiver: string) {
  const [heard, setHeard] = useState<Record<string, number>>({});
  const [hearing, setHearing] = useState<Record<string, number>>({});
  useEffect(() => {
    let current = true;
    const poll = async () => {
      try {
        const [activity, found] = await Promise.all([
          invoke<BandActivity[]>("band_activity", { minutes: NOW_MINUTES }),
          invoke<HearingYourArea>("hearing_your_area", { minutes: NOW_MINUTES, band: null, receiver }),
        ]);
        if (!current) return;
        setHeard(Object.fromEntries(activity.map((a) => [a.band, a.uniqueCallsigns])));
        const byBand: Record<string, number> = {};
        for (const s of found.stations) byBand[s.band] = (byBand[s.band] ?? 0) + 1;
        setHearing(byBand);
      } catch {
        // The heard columns stay empty; the model does not depend on them.
      }
    };
    poll();
    const timer = setInterval(poll, POLL_MS);
    return () => {
      current = false;
      clearInterval(timer);
    };
  }, [receiver]);
  return { heard, hearing };
}

/** Which band reaches the most stations from the log, at the hour shown and through the day. */
export function MostContacts(props: Props) {
  const { txPosition, year, month, ssn, txStation, rxStation, mode, modeLabel, reliability, clockHour, nowClock, zone } =
    props;
  const [timeOfDay, setTimeOfDay] = useState(false);
  const [days, setDays] = useState(0);
  const [report, setReport] = useState<ContactsReport | null>(null);
  const [day, setDay] = useState<ContactsReport | null>(null);
  const [busy, setBusy] = useState<"hour" | "day" | null>(null);
  const [progress, setProgress] = useState<Progress | null>(null);
  const [error, setError] = useState("");
  const now = useHeardNow(txPosition);

  const area = useMemo(
    () => ({
      txPosition,
      year,
      month,
      ssn,
      txStation,
      rxStation,
      mode,
      requiredReliabilityPct: reliability,
      utcHour: 1,
    }),
    [txPosition, year, month, ssn, txStation, rxStation, mode, reliability],
  );
  const settings = JSON.stringify({ area, timeOfDay, days });

  useEffect(() => {
    let active = true;
    const unlisten = listen<Progress>("contacts-progress", (event) => {
      if (active) setProgress(event.payload);
    });
    return () => {
      active = false;
      unlisten.then((stop) => stop()).catch(() => {});
    };
  }, []);

  // The day's table describes the settings it was made with.
  useEffect(() => setDay(null), [settings]);

  useEffect(() => {
    if (!txPosition.trim()) return;
    let current = true;
    setBusy("hour");
    setError("");
    invoke<ContactsReport>("most_contacts", {
      query: { area, hours: [clockHour], timeOfDay, historyDays: days || null },
    })
      .then((next) => current && setReport(next))
      .catch((e) => current && setError(String(e)))
      .finally(() => current && setBusy(null));
    return () => {
      current = false;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [settings, clockHour]);

  const workOutDay = async () => {
    setBusy("day");
    setProgress(null);
    setError("");
    try {
      setDay(
        await invoke<ContactsReport>("most_contacts", {
          query: { area, hours: ALL_HOURS, timeOfDay, historyDays: days || null },
        }),
      );
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(null);
      setProgress(null);
    }
  };

  const hour = report?.hours.find((h) => h.clockHour === clockHour) ?? null;
  const ranked = hour ? [...hour.bands].sort((a, b) => b.expected - a.expected) : [];
  const best = ranked[0];
  const counted = hour?.stations ?? 0;
  const span = SPANS.find((s) => s.days === days)?.label ?? "";

  return (
    <section>
      <div className="view-head">
        <h2>Most contacts at {hourBoth(clockHour, zone)}</h2>
        <span className="hint">
          {modeLabel}, from {txPosition.trim() || "your position"}, the stations in your log
        </span>
      </div>

      <div className="controls">
        <label className="inline">
          Count
          <select value={timeOfDay ? "hour" : "any"} onChange={(e) => setTimeOfDay(e.target.value === "hour")}>
            <option value="any">every station heard</option>
            <option value="hour">stations usually on at this hour</option>
          </select>
        </label>
        <label className="inline">
          from
          <select value={days} onChange={(e) => setDays(Number(e.target.value))}>
            {SPANS.map((s) => (
              <option key={s.days} value={s.days}>
                {s.label}
              </option>
            ))}
          </select>
        </label>
        {busy === "hour" && <span className="hint">Predicting…</span>}
      </div>
      {error && <p className="error">{error}</p>}
      {!txPosition.trim() && <p className="note">Enter your position in the From field first.</p>}

      {report && report.stationsInLog === 0 && (
        <p className="note">
          No station in your log has sent a locator yet. Turn on listening in Heard, or add a WSJT-X log there.
        </p>
      )}

      {hour && best && counted > 0 && (
        <>
          <p className="lead">
            Try <span className="band-name">{best.band}</span> first: of the {counted.toLocaleString()} stations
            counted, it should reach about {round(best.expected)} on a typical day
            {ranked.length > 2 && (
              <>
                , then <span className="band-name">{ranked[1].band}</span> ({round(ranked[1].expected)}) and{" "}
                <span className="band-name">{ranked[2].band}</span> ({round(ranked[2].expected)})
              </>
            )}
            .
          </p>
          {timeOfDay && !hour.timeOfDay && (
            <p className="caution">
              Your log has only {hour.listenedHours} {hour.listenedHours === 1 ? "hour" : "hours"} of listening around
              this time of day, too little to tell who is usually on, so every station is counted.
            </p>
          )}

          <div className="scroll-x">
            <table className="bands wide contacts">
              <thead>
                <tr>
                  <th className="left">Band</th>
                  <th>Should reach</th>
                  <th className="left">Share of those counted</th>
                  {DISTANCES.map((d) => (
                    <th key={d} className="dist-col">
                      {d}
                    </th>
                  ))}
                  <th>Heard, last hour</th>
                  <th>Hear your area</th>
                </tr>
              </thead>
              <tbody>
                {ranked.map((b, i) => (
                  <tr key={b.band} className={i === 0 ? "top" : undefined}>
                    <th className="band">{b.band}</th>
                    <td className="figure">{round(b.expected)}</td>
                    <td className="left">
                      <Meter value={b.expected / counted} ticks={[]} />
                    </td>
                    {b.byDistance.map((value, column) => (
                      <td key={column} className={`dist-col num${value < 0.5 ? " hint" : ""}`}>
                        {round(value)}
                      </td>
                    ))}
                    <td className="num">
                      {now.heard[b.band] === undefined ? <span className="hint">—</span> : now.heard[b.band]}
                    </td>
                    <td className="num">
                      {now.hearing[b.band] ? (
                        <>
                          <Ring /> {now.hearing[b.band]}
                        </>
                      ) : (
                        <span className="hint">—</span>
                      )}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>

          <p className="note">
            Should reach: the stations in your log, each placed by the locator it last sent, summed over the
            share of days the model gives a {modeLabel} signal from you to them at {hourBoth(clockHour, zone)}.
            Counted: {hour.timeOfDay ? "stations heard within an hour of this time of day, on any day and band" : "every station heard"},
            from {span}, {counted.toLocaleString()} in all
            {report && report.tooNear > 0 && `; ${report.tooNear} within 100 km are left out, since the model covers sky wave only`}.
            Each is assumed to run a station like "{rxStation.name}". The last two columns are what is happening now,
            whatever hour is shown: callsigns heard on the band in the last hour, and stations heard reporting you or a
            station near you.
          </p>
          <p className="note">
            The stations are the ones your receiver could hear, so places it never hears are missing, and the log is
            of FT8 stations even when the mode is voice. A band that reaches many stations can also be crowded: once
            plenty are workable, your rate is set by the time each contact takes, not by reach.
          </p>
        </>
      )}

      <div className="section-title" style={{ marginTop: 18 }}>
        <h3>Through the day</h3>
        {!day && (
          <button type="button" onClick={workOutDay} disabled={busy !== null || !txPosition.trim()}>
            {busy === "day" ? "Working out…" : "Work out every hour"}
          </button>
        )}
        {busy === "day" && progress && progress.total > 0 && (
          <span className="hint">
            {progress.done} of {progress.total} hours predicted
          </span>
        )}
      </div>
      {!day && busy !== "day" && (
        <p className="note">
          Every hour of the day for every band. The first time for a month and set of stations takes about half a
          minute; after that it is kept.
        </p>
      )}
      {day && <DayTable day={day} clockHour={clockHour} nowClock={nowClock} zone={zone} onSelectHour={props.onSelectHour} />}
    </section>
  );
}

function DayTable({
  day,
  clockHour,
  nowClock,
  zone,
  onSelectHour,
}: {
  day: ContactsReport;
  clockHour: number;
  nowClock: number;
  zone: Zone;
  onSelectHour: (clockHour: number) => void;
}) {
  const hours = [...day.hours].sort((a, b) => a.clockHour - b.clockHour);
  const bands = hours[0]?.bands.map((b) => b.band) ?? [];
  const anyTimeOfDay = hours.some((h) => h.timeOfDay);
  return (
    <>
      <div className="scroll-x">
        <table className="results day-reach">
          <colgroup>
            <col className="band-col" />
            {hours.map((h) => (
              <col key={h.clockHour} />
            ))}
          </colgroup>
          <thead>
            <tr>
              <th className="left">UTC</th>
              {hours.map((h) => (
                <th
                  key={h.clockHour}
                  className={`hour${h.clockHour === clockHour ? " shown" : ""}${h.clockHour === nowClock ? " now" : ""}`}
                  onClick={() => onSelectHour(h.clockHour)}
                  title={`Show ${String(h.clockHour).padStart(2, "0")} UTC`}
                >
                  {String(h.clockHour).padStart(2, "0")}
                </th>
              ))}
            </tr>
            <tr>
              <th className="left hint">{zone.name}</th>
              {hours.map((h) => (
                <th key={h.clockHour} className="hint">
                  {localHour(h.clockHour, zone)}
                </th>
              ))}
            </tr>
          </thead>
          <tbody>
            {bands.map((band, row) => (
              <tr key={band}>
                <th className="band">{band}</th>
                {hours.map((h) => {
                  const value = h.bands[row].expected;
                  const share = h.stations > 0 ? value / h.stations : 0;
                  return (
                    <td
                      key={h.clockHour}
                      className={`cell${h.clockHour === clockHour ? " shown" : ""}`}
                      style={relStyle(share)}
                      title={`${band} at ${String(h.clockHour).padStart(2, "0")} UTC: about ${round(value)} of ${h.stations} stations`}
                    >
                      {round(value)}
                    </td>
                  );
                })}
              </tr>
            ))}
            <tr className="counted">
              <th className="left hint">Counted</th>
              {hours.map((h) => (
                <td key={h.clockHour} className="num hint">
                  {h.stations}
                  {anyTimeOfDay && h.timeOfDay && "*"}
                </td>
              ))}
            </tr>
          </tbody>
        </table>
      </div>
      <div className="legend">
        <RelScale label="Share of those counted" />
        <span>Numbers are stations; click an hour to show it everywhere.</span>
      </div>
      {anyTimeOfDay && (
        <p className="note">
          * Only stations usually on at that hour were counted. Elsewhere the log has too little listening around that
          time of day, so every station was counted.
        </p>
      )}
    </>
  );
}
