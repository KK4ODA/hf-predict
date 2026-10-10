import { PathDetail } from "./types";
import { clockHour } from "./tiers";
import { hourBoth, Zone } from "./localtime";
import { pct, relStyle } from "./ui";

/** Reliability on each band at each transmit power, for one hour. */
export function PowerTable({ detail, hourIndex, zone }: { detail: PathDetail; hourIndex: number; zone: Zone }) {
  const prediction = detail.prediction;
  const hour = prediction.run.prediction.hours[hourIndex];
  const bands = prediction.bands.map((band, i) => ({ band, i })).sort((a, b) => b.band.mhz - a.band.mhz);

  return (
    <section>
      <h3>Effect of transmit power at {hourBoth(clockHour(hour.utcHour), zone)}</h3>
      <table className="results">
        <thead>
          <tr>
            <th className="left">Band</th>
            {detail.power.map((p) => (
              <th key={p.powerWatts}>{p.powerWatts} W</th>
            ))}
          </tr>
        </thead>
        <tbody>
          {bands.map(({ band, i }) => (
            <tr key={band.name}>
              <th className="band">{band.name}</th>
              {detail.power.map((p) => {
                const reliability = p.reliability[hourIndex][i];
                return (
                  <td
                    key={p.powerWatts}
                    className="cell"
                    style={relStyle(reliability)}
                    title={`SNR ${p.snrDb[hourIndex][i]} dB-Hz at ${p.powerWatts} W`}
                  >
                    {pct(reliability)}
                  </td>
                );
              })}
            </tr>
          ))}
        </tbody>
      </table>
      <p className="note">
        Reliability with everything else unchanged. Each tenfold increase in power adds 10 dB of SNR.
      </p>
    </section>
  );
}
