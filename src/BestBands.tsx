import { PathDetail } from "./types";
import { clockHour, hourLabel, openWindows, tierOf, TIER_LEGEND } from "./tiers";

type Props = {
  detail: PathDetail;
  /** The other way round the earth, to flag bands where it does better. */
  other: PathDetail;
  otherName: string;
  hourIndex: number;
};

/** How much better the other path must be before it is worth mentioning. */
const OTHER_PATH_MARGIN = 0.1;

/** Bands ranked for one hour, best first, with the hours each is worth trying. */
export function BestBands({ detail, other, otherName, hourIndex }: Props) {
  const prediction = detail.prediction.run.prediction;
  const hour = prediction.hours[hourIndex];
  const otherHour = other.prediction.run.prediction.hours[hourIndex];

  const rows = detail.prediction.bands
    .map((band, i) => {
      const byClockHour = new Array<number>(24).fill(0);
      for (const h of prediction.hours) byClockHour[clockHour(h.utcHour)] = h.frequencies[i].reliability;
      return {
        band,
        f: hour.frequencies[i],
        otherReliability: otherHour.frequencies[i].reliability,
        windows: openWindows(byClockHour),
      };
    })
    .sort((a, b) => b.f.reliability - a.f.reliability || b.f.snrDb - a.f.snrDb);

  return (
    <section>
      <h3>Best bands at {hourLabel(hour.utcHour)} UTC</h3>
      <table className="bands">
        <thead>
          <tr>
            <th>Band</th>
            <th>Outlook</th>
            <th>Reliability</th>
            <th>SNR</th>
            <th>Mode, angle</th>
            <th>Worth trying (UTC)</th>
          </tr>
        </thead>
        <tbody>
          {rows.map(({ band, f, otherReliability, windows }) => {
            const tier = tierOf(f.reliability);
            return (
              <tr key={band.name}>
                <th>{band.name}</th>
                <td>
                  <span className={`tier tier-${tier.key}`} aria-hidden="true">
                    {tier.icon}
                  </span>{" "}
                  {tier.label}
                </td>
                <td>{(f.reliability * 100).toFixed(0)}%</td>
                <td>{f.snrDb.toFixed(0)} dB-Hz</td>
                <td>
                  {f.mode}, {f.takeoffAngleDeg.toFixed(0)}°
                </td>
                <td className="left">
                  {windows.length > 0 ? windows.join(", ") : "—"}
                  {otherReliability - f.reliability >= OTHER_PATH_MARGIN && (
                    <span className="hint">
                      {" "}
                      · {otherName} path better now ({(otherReliability * 100).toFixed(0)}%)
                    </span>
                  )}
                </td>
              </tr>
            );
          })}
        </tbody>
      </table>
      <p className="note">
        Reliability is the share of days in the month on which the required SNR is met.{" "}
        {TIER_LEGEND} "Worth trying" lists the hours at Fair or better.
      </p>
    </section>
  );
}
