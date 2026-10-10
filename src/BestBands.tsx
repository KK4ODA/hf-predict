import { PathDetail } from "./types";
import { byClockHour, clockHour, describeWindows, FAIR_RELIABILITY, tierOf, TIER_LEGEND, Windows } from "./tiers";
import { hourBoth, Zone } from "./localtime";
import { HourStrip, Meter, pct, RelScale } from "./ui";

type Props = {
  detail: PathDetail;
  /** The other way round the earth, to flag bands where it does better. */
  other: PathDetail;
  otherName: string;
  hourIndex: number;
  zone: Zone;
  nowClock: number;
  modeLabel: string;
  longPath: boolean;
};

/** How much better the other path must be before it is worth mentioning. */
const OTHER_PATH_MARGIN = 0.1;

export function WindowsText({ windows, none = "none" }: { windows: Windows | null; none?: string }) {
  if (!windows) return <span className="hint">{none}</span>;
  if (windows.allDay) return <>all day</>;
  return (
    <>
      <span className="num">{windows.utc}</span> UTC{" "}
      <span className="hint">
        ({windows.local} {windows.zone})
      </span>
    </>
  );
}

/** Bands ranked for the hour shown, with how each fares through the day. */
export function BestBands({ detail, other, otherName, hourIndex, zone, nowClock, modeLabel, longPath }: Props) {
  const prediction = detail.prediction.run.prediction;
  const hour = prediction.hours[hourIndex];
  const clock = clockHour(hour.utcHour);
  const otherHour = other.prediction.run.prediction.hours[hourIndex];
  const required = detail.prediction.requiredSnrDbHz;

  const rows = detail.prediction.bands
    .map((band, i) => {
      const daily = byClockHour(prediction.hours, i);
      return {
        band,
        f: hour.frequencies[i],
        daily,
        otherReliability: otherHour.frequencies[i].reliability,
        windows: describeWindows(daily, zone),
      };
    })
    .sort((a, b) => b.f.reliability - a.f.reliability || b.f.snrDb - a.f.snrDb);

  const workable = rows.filter((r) => r.f.reliability >= FAIR_RELIABILITY);
  const best = rows[0];

  return (
    <section>
      <div className="view-head">
        <h2>Best bands at {hourBoth(clock, zone)}</h2>
        <span className="hint">
          {modeLabel}, {longPath ? "long" : "short"} path
        </span>
      </div>

      <p className="lead">
        {workable.length > 0 ? (
          <>
            Try <span className="band-name">{workable[0].band.name}</span> first, predicted on{" "}
            {pct(workable[0].f.reliability)} of days
            {workable.length > 1 && (
              <>
                , then{" "}
                {workable.slice(1, 3).map((r, i) => (
                  <span key={r.band.name}>
                    {i > 0 && " and "}
                    <span className="band-name">{r.band.name}</span>
                  </span>
                ))}
              </>
            )}
            .
          </>
        ) : (
          <>
            No band reaches fair reliability at this hour. The best is{" "}
            <span className="band-name">{best.band.name}</span> on {pct(best.f.reliability)} of days; the
            strips below show when each band opens.
          </>
        )}
      </p>

      <div className="scroll-x">
        <table className="bands wide">
          <thead>
            <tr>
              <th className="left">Band</th>
              <th className="left">Outlook</th>
              <th>Reliability</th>
              <th>SNR</th>
              <th className="left">Through the day, UTC</th>
              <th className="left">Worth trying</th>
              <th className="opt-col">Mode, angle</th>
            </tr>
          </thead>
          <tbody>
            {rows.map(({ band, f, daily, otherReliability, windows }, i) => {
              const tier = tierOf(f.reliability);
              const margin = f.snrDb - required;
              return (
                <tr key={band.name} className={i === 0 && f.reliability >= FAIR_RELIABILITY ? "top" : undefined}>
                  <th className="band">{band.name}</th>
                  <td className="left">
                    <span className={`tier tier-${tier.key}`} aria-hidden="true">
                      {tier.icon}
                    </span>{" "}
                    {tier.label}
                  </td>
                  <td>
                    <Meter value={f.reliability} />
                  </td>
                  <td className="snr" title={`Required ${required} dB-Hz for ${modeLabel}`}>
                    <span className="num">{f.snrDb.toFixed(0)}</span> dB-Hz
                    <span className="need">
                      {margin >= 0 ? `${margin.toFixed(0)} to spare` : `${(-margin).toFixed(0)} short`}
                    </span>
                  </td>
                  <td className="left">
                    <HourStrip
                      byClockHour={daily}
                      selected={clock}
                      now={nowClock}
                      label={`${band.name} reliability by UTC hour`}
                    />
                  </td>
                  <td className="windows">
                    <WindowsText windows={windows} />
                    {otherReliability - f.reliability >= OTHER_PATH_MARGIN && (
                      <div className="hint">
                        {otherName} path better now, {pct(otherReliability)}
                      </div>
                    )}
                  </td>
                  <td className="hint opt-col">
                    {f.mode} {f.takeoffAngleDeg.toFixed(0)}°
                  </td>
                </tr>
              );
            })}
          </tbody>
        </table>
      </div>

      <div className="legend">
        <RelScale />
        <span>Strips run 00 to 23 UTC; the outlined hour is the one shown, the notch marks now.</span>
      </div>
      <p className="note">
        Reliability is the share of days in the month on which the signal meets the {required} dB-Hz{" "}
        {modeLabel} needs; the ticks on each bar mark 30% and 70%. {TIER_LEGEND} "Worth trying" lists
        the hours at fair or better. SNR is the monthly median.
      </p>
    </section>
  );
}
