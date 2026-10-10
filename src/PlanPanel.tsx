import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { hourBoth, localClock, Zone } from "./localtime";
import { ListenerStatus, Plan, StationProfile } from "./types";

type Props = {
  txPosition: string;
  rxPosition: string;
  year: number;
  month: number;
  ssn: number | null;
  txStation: StationProfile;
  rxStation: StationProfile;
  clockHour: number;
  zone: Zone;
};

const BANDS = ["80 m", "60 m", "40 m", "30 m", "20 m", "17 m", "15 m", "12 m", "10 m"];
const LENGTHS = [
  { minutes: 15, label: "15 minutes" },
  { minutes: 30, label: "30 minutes" },
  { minutes: 60, label: "1 hour" },
  { minutes: 120, label: "2 hours" },
];
const TIER_LABEL = { long: "long", standard: "standard", probe: "probe" };
const OBSERVED_LABEL = {
  strong: "many stations",
  moderate: "some stations",
  limited: "little",
  none: "nothing",
  notSampled: "not listened to",
};
const STATUS_POLL_MS = 5000;

const mmss = (seconds: number) => {
  const s = Math.max(0, Math.round(seconds));
  return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}`;
};
const mhz = (hz: number) => (hz / 1e6).toFixed(3);
const ago = (minutes: number | null) => {
  if (minutes === null) return "never";
  if (minutes < 1) return "now";
  if (minutes < 90) return `${Math.round(minutes)} min ago`;
  if (minutes < 48 * 60) return `${Math.round(minutes / 60)} h ago`;
  return `${Math.round(minutes / 1440)} days ago`;
};

/** Which bands to listen on and when, and help following the plan by hand. */
export function PlanPanel(props: Props) {
  const { txPosition, rxPosition, year, month, ssn, txStation, rxStation, clockHour, zone } = props;
  const [minutes, setMinutes] = useState(30);
  const [aim, setAim] = useState(true);
  const [excluded, setExcluded] = useState<string[]>([]);
  const [plan, setPlan] = useState<Plan | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [startedAt, setStartedAt] = useState<number | null>(null);
  const [now, setNow] = useState(Date.now());
  const [onBand, setOnBand] = useState<string | null>(null);

  const aiming = aim && rxPosition.trim() !== "";

  // While following: a clock tick, and which band WSJT-X is on.
  useEffect(() => {
    if (startedAt === null) return;
    const tick = setInterval(() => setNow(Date.now()), 1000);
    let active = true;
    const poll = () =>
      invoke<ListenerStatus>("listener_status")
        .then((s) => active && setOnBand(s.tracker?.decoders[0]?.band ?? null))
        .catch(() => active && setOnBand(null));
    poll();
    const status = setInterval(poll, STATUS_POLL_MS);
    return () => {
      active = false;
      clearInterval(tick);
      clearInterval(status);
    };
  }, [startedAt]);

  const make = async () => {
    setBusy(true);
    setError("");
    setStartedAt(null);
    try {
      setPlan(
        await invoke<Plan>("listen_plan", {
          query: {
            txPosition,
            destination: aiming ? rxPosition : null,
            year,
            month,
            ssn,
            txStation,
            rxStation,
            clockHour,
            minutes,
            excludedBands: excluded,
          },
        }),
      );
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  const elapsed = startedAt === null ? null : (now - startedAt) / 1000;
  const current =
    plan && elapsed !== null
      ? plan.items.find((i) => elapsed >= i.startS && elapsed < i.startS + i.dwellS) ?? null
      : null;
  const next = plan && current ? plan.items[plan.items.indexOf(current) + 1] ?? null : null;
  const finished = plan && elapsed !== null && elapsed >= plan.minutes * 60;

  return (
    <section>
      <div className="controls">
        <label className="inline">
          <input type="checkbox" checked={aiming} disabled={rxPosition.trim() === ""} onChange={(e) => setAim(e.target.checked)} />
          Aim at the To position{rxPosition.trim() === "" && " (none set)"}
        </label>
        <label className="inline">
          Plan for
          <select value={minutes} onChange={(e) => setMinutes(Number(e.target.value))}>
            {LENGTHS.map((l) => (
              <option key={l.minutes} value={l.minutes}>
                {l.label}
              </option>
            ))}
          </select>
        </label>
        <button type="button" onClick={make} disabled={busy || txPosition.trim() === ""}>
          {busy ? "Predicting…" : "Make a plan"}
        </button>
      </div>
      <div className="controls">
        <span className="hint">Leave out:</span>
        {BANDS.map((band) => (
          <label key={band} className="inline">
            <input
              type="checkbox"
              checked={excluded.includes(band)}
              onChange={(e) =>
                setExcluded(e.target.checked ? [...excluded, band] : excluded.filter((b) => b !== band))
              }
            />
            {band}
          </label>
        ))}
      </div>
      {error && <p className="error">{error}</p>}
      {txPosition.trim() === "" && <p className="note">Enter your position in the From field first.</p>}

      {plan === null && !busy && (
        <p className="note">
          A listening plan says which bands to listen on, in what order and for how long, so that
          what you hear covers more than the band WSJT-X happens to be on. It weighs what the model
          predicts for {aiming ? "the path to the To position" : "the world from your position"} at{" "}
          {hourBoth(clockHour, zone)}, what has been heard in the last hour, and how long each band
          has gone without being listened to. Nothing here transmits or touches the radio.
        </p>
      )}

      {plan && (
        <>
          <h3>Bands to tick in WSJT-X band hopping</h3>
          <p>
            {plan.hopBands.map((band, i) => (
              <span key={band}>
                {i > 0 && ", "}
                <strong>{band}</strong>
              </span>
            ))}
            {plan.hopBands.length === 0 && "none; every band is left out"}
          </p>
          <p className="note">
            WSJT-X hops between the ticked bands on its own rhythm and stops while transmit is
            enabled; it cannot be told how long to stay. The schedule below is for following by
            hand, or for a later version that moves the radio itself. One pass over every band
            takes {mmss(plan.cycleS)} and the plan repeats it for {plan.minutes} minutes.
          </p>

          <h3>Schedule</h3>
          <div className="controls">
            {startedAt === null ? (
              <button type="button" onClick={() => { setNow(Date.now()); setStartedAt(Date.now()); }} disabled={plan.items.length === 0}>
                Start following now
              </button>
            ) : (
              <button type="button" onClick={() => setStartedAt(null)}>
                Stop
              </button>
            )}
          </div>
          {current && (
            <p className="plan-now">
              Now <strong>{current.band}</strong> ({mhz(current.dialHz)} MHz), {mmss(current.startS + current.dwellS - (elapsed ?? 0))} left
              {next && (
                <>
                  {" "}· then {next.band} ({mhz(next.dialHz)} MHz)
                </>
              )}
              <br />
              <span className={onBand === current.band ? "hint" : "caution"}>
                {onBand === null
                  ? "WSJT-X is not reporting its band"
                  : onBand === current.band
                    ? `WSJT-X is on ${onBand} ✓`
                    : `WSJT-X is on ${onBand}: switch to ${current.band}`}
              </span>
            </p>
          )}
          {finished && <p className="banner">The plan has run its course. Make a new one to carry on.</p>}
          <div className="scroll-x">
            <table className="results compact">
              <thead>
                <tr>
                  <th>#</th>
                  <th>Band</th>
                  <th>Dial</th>
                  <th>Starts</th>
                  {startedAt !== null && <th>At</th>}
                  <th>Stay</th>
                  <th>Dwell</th>
                </tr>
              </thead>
              <tbody>
                {plan.items.map((item, i) => (
                  <tr key={i} className={current === item ? "selected" : undefined}>
                    <th>{i + 1}</th>
                    <td>{item.band}</td>
                    <td>{mhz(item.dialHz)} MHz</td>
                    <td>+{mmss(item.startS)}</td>
                    {startedAt !== null && <td>{localClock(startedAt / 1000 + item.startS, false)}</td>}
                    <td>{mmss(item.dwellS)}</td>
                    <td>{TIER_LABEL[item.tier]}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>

          <h3>Why this order</h3>
          <div className="scroll-x">
            <table className="results compact">
              <thead>
                <tr>
                  <th>Band</th>
                  <th>Dwell</th>
                  <th>Predicted</th>
                  <th>Heard, last hour</th>
                  <th>Last listened</th>
                  <th>Priority</th>
                </tr>
              </thead>
              <tbody>
                {plan.bands.map((b) => (
                  <tr key={b.band} className={b.tier === null ? "muted" : undefined}>
                    <th>{b.band}</th>
                    <td>{b.tier === null ? "left out" : TIER_LABEL[b.tier]}</td>
                    <td>{(b.prediction * 100).toFixed(0)}%</td>
                    <td className="left">{OBSERVED_LABEL[b.observed]}</td>
                    <td>{ago(b.minutesSinceListened)}</td>
                    <td>{b.priority.toFixed(2)}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
          <p className="note">
            Predicted is {aiming ? "the FT8 reliability to the To position" : "the share of the world an FT8 signal should reach"} at {hourBoth(clockHour, zone)}.
            Priority adds that to a bonus for what was heard in the last hour (up to 0.5) and to how
            long the band has gone unheard (up to 1 after an hour). The top three bands get long
            dwells (8 periods), the next two standard (4), the rest a probe (2); each dwell counts
            one extra period for retuning. Every band comes round at least once in 20 minutes
            however poor its prediction, so a surprise opening is never missed for long.
          </p>
        </>
      )}
    </section>
  );
}
