// Local time shown beside UTC, in the computer's zone. Predictions are for a
// month, so their offset is taken at the middle of that month; a live time
// uses its own instant, so daylight-saving changes are followed.

export type Zone = {
  /** Minutes east of UTC. */
  offset: number;
  /** Short name, e.g. "EDT" or "GMT+2". */
  name: string;
};

const two = (n: number) => String(n).padStart(2, "0");

export function zoneAt(date: Date): Zone {
  const name =
    new Intl.DateTimeFormat(undefined, { timeZoneName: "short" })
      .formatToParts(date)
      .find((p) => p.type === "timeZoneName")?.value ?? "local";
  return { offset: -date.getTimezoneOffset(), name };
}

/** The zone at noon UTC on the 15th, standing for a predicted month. */
export function zoneForMonth(year: number, month: number): Zone {
  return zoneAt(new Date(Date.UTC(year, month - 1, 15, 12)));
}

/** A UTC clock hour (0–23) in local time: "09", or "09:30" in a half-hour zone. */
export function localHour(clock: number, zone: Zone): string {
  const minutes = (((clock * 60 + zone.offset) % 1440) + 1440) % 1440;
  const rest = minutes % 60;
  return two(Math.floor(minutes / 60)) + (rest === 0 ? "" : `:${two(rest)}`);
}

/** "13 UTC (09 EDT)" */
export function hourBoth(clock: number, zone: Zone): string {
  return `${two(clock)} UTC (${localHour(clock, zone)} ${zone.name})`;
}

/** "09:31:15" in local time. */
export function localClock(unixSeconds: number, withSeconds = true): string {
  const d = new Date(unixSeconds * 1000);
  const parts = [d.getHours(), d.getMinutes(), ...(withSeconds ? [d.getSeconds()] : [])];
  return parts.map(two).join(":");
}

/** "09:31 UTC (05:31 EDT)" */
export function clockBoth(unixSeconds: number): string {
  const utc = new Date(unixSeconds * 1000).toISOString().slice(11, 16);
  return `${utc} UTC (${localClock(unixSeconds, false)} ${zoneAt(new Date(unixSeconds * 1000)).name})`;
}

/** "2026-10-04 19:58 UTC (15:58 EDT)", with the local date too when it differs. */
export function stampBoth(unixSeconds: number): string {
  const d = new Date(unixSeconds * 1000);
  const iso = d.toISOString();
  const utcDate = iso.slice(0, 10);
  const localDate = `${d.getFullYear()}-${two(d.getMonth() + 1)}-${two(d.getDate())}`;
  const localDay = localDate === utcDate ? "" : `${localDate} `;
  return `${utcDate} ${iso.slice(11, 16)} UTC (${localDay}${localClock(unixSeconds, false)} ${zoneAt(d).name})`;
}
