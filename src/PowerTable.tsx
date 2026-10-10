import { PathDetail } from "./types";
import { clockHour, shade } from "./tiers";
import { hourBoth, Zone } from "./localtime";

/** Reliability on each band at each transmit power, for one hour. */
export function PowerTable({ detail, hourIndex, zone }: { detail: PathDetail; hourIndex: number; zone: Zone }) {
  const prediction = detail.prediction;
  const hour = prediction.run.prediction.hours[hourIndex];

  return (
    <section>
      <h3>Effect of transmit power at {hourBoth(clockHour(hour.utcHour), zone)}</h3>
      <table className="results">
        <thead>
          <tr>
            <th>Band</th>
            {detail.power.map((p) => (
              <th key={p.powerWatts}>{p.powerWatts} W</th>
            ))}
          </tr>
        </thead>
        <tbody>
          {prediction.bands.map((band, i) => (
            <tr key={band.name}>
              <th>{band.name}</th>
              {detail.power.map((p) => {
                const reliability = p.reliability[hourIndex][i];
                return (
                  <td
                    key={p.powerWatts}
                    style={{ background: shade(reliability) }}
                    title={`SNR ${p.snrDb[hourIndex][i]} dB-Hz at ${p.powerWatts} W`}
                  >
                    {(reliability * 100).toFixed(0)}%
                  </td>
                );
              })}
            </tr>
          ))}
        </tbody>
      </table>
      <p className="note">
        Reliability with everything else unchanged. Each tenfold increase in power adds 10 dB of
        SNR.
      </p>
    </section>
  );
}
