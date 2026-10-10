import { localHour, stampBoth, Zone, zoneAt } from "./localtime";
import { ChangeEvent, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { age } from "./ui";
import {
  Conditions,
  ConditionsUpdate,
  FetchResult,
  Imported,
  Outlook27,
  ProductStatus,
  Sgas,
  SsnTableStatus,
  SsnUsed,
  ThreeDay,
  Transport,
  Wwv,
} from "./types";

const TRANSPORT: Record<Transport, string> = {
  internet: "Internet",
  winlink: "Winlink",
  pasted: "pasted text",
  file: "a file",
};

const OUTCOME: Record<Imported["outcome"], string> = {
  stored: "imported",
  alreadyHave: "already had this issue",
  olderThanStored: "older than the stored copy, ignored",
  notUnderstood: "not understood",
};

/** Kp at or above this is a geomagnetic storm; each step up is one G level. */
const STORM_KP = 5;

/** A forecast period such as "00-03UT" as "00–03 (20–23)", local hours in brackets. */
function period(text: string, zone: Zone): string {
  const [from, to] = text.replace("UT", "").split("-").map(Number);
  if (Number.isNaN(from) || Number.isNaN(to)) return text;
  const two = (h: number) => String(h).padStart(2, "0");
  return `${two(from)}–${two(to)} (${localHour(from, zone)}–${localHour(to % 24, zone)})`;
}

const shown = (value: number | string | null) => (value === null ? "not reported" : String(value));

function kpText(kp: number | null): string {
  if (kp === null) return "—";
  return kp >= STORM_KP ? `${kp} G${Math.min(5, Math.floor(kp) - 4)}` : String(kp);
}

function kWords(k: number | null): string {
  if (k === null) return "";
  if (k >= 5) return "storm";
  if (k >= 4) return "active";
  if (k >= 3) return "unsettled";
  return "quiet";
}

function aWords(a: number | null): string {
  if (a === null) return "";
  if (a >= 30) return "storm";
  if (a >= 16) return "active";
  if (a >= 8) return "unsettled";
  return "quiet";
}

function fluxWords(flux: number | null): string {
  if (flux === null) return "";
  if (flux >= 150) return "high; upper bands favoured";
  if (flux >= 100) return "moderate";
  return "low; upper bands weak";
}

type ReadingProps = {
  label: string;
  value: string | number | null;
  scale: string;
  provenance: string;
  state: "current" | "stale" | "none";
};

/** One instrument reading: what, how much, what that means, where from. */
function Reading({ label, value, scale, provenance, state }: ReadingProps) {
  return (
    <div className={`reading ${state === "current" ? "" : state}`}>
      <span className="label">{label}</span>
      <span className="value">{value ?? "—"}</span>
      <span className="scale">{scale || " "}</span>
      <span className="prov">{provenance}</span>
    </div>
  );
}

function Badge({ state }: { state: "current" | "stale" | "none" }) {
  const text = { current: "Current", stale: "Old", none: "No data" }[state];
  return <span className={`badge badge-${state}`}>{text}</span>;
}

/** Kp forecast as columns, one per three-hour period, grouped by day. */
function KpChart({ p }: { p: ThreeDay }) {
  const W = 760;
  const H = 150;
  const left = 30;
  const bottom = 34;
  const top = 10;
  const periods = p.kp.length;
  const columns = p.days.length * periods;
  const slot = (W - left) / Math.max(1, columns);
  const bar = Math.min(16, slot * 0.6);
  const y = (kp: number) => top + (H - top - bottom) * (1 - kp / 9);
  return (
    <div className="chart" style={{ maxWidth: W }}>
      <svg className="kpbars" viewBox={`0 0 ${W} ${H}`} role="img" aria-label="Kp forecast by three-hour period">
        {[0, 3, 5, 9].map((k) => (
          <g key={k}>
            <line className="grid" x1={left} x2={W} y1={y(k)} y2={y(k)} />
            <text className="tick" x={left - 6} y={y(k) + 4} textAnchor="end">
              {k}
            </text>
          </g>
        ))}
        {p.days.map((day, d) => (
          <g key={day}>
            {p.kp.map((row, r) => {
              const kp = row.values[d];
              const i = d * periods + r;
              const x = left + i * slot + (slot - bar) / 2;
              return kp === null ? null : (
                <rect
                  key={row.period}
                  className={`kp${kp >= STORM_KP ? " g5" : kp >= 4 ? " g4" : ""}`}
                  x={x}
                  y={y(kp)}
                  width={bar}
                  height={y(0) - y(kp)}
                  rx={1.5}
                >
                  <title>
                    {day} {row.period}: Kp {kpText(kp)}
                  </title>
                </rect>
              );
            })}
            <text className="tick" x={left + (d + 0.5) * periods * slot} y={H - 12} textAnchor="middle">
              {day}
            </text>
            {d > 0 && <line className="axis" x1={left + d * periods * slot} x2={left + d * periods * slot} y1={top} y2={y(0)} />}
          </g>
        ))}
        <line className="axis" x1={left} x2={W} y1={y(0)} y2={y(0)} />
      </svg>
    </div>
  );
}

function ThreeDayBody({ p }: { p: ThreeDay }) {
  const zone = zoneAt(new Date());
  const percent = (values: (number | null)[]) => values.map((v, i) => <td key={i}>{v === null ? "—" : `${v}%`}</td>);
  return (
    <>
      <KpChart p={p} />
      <div className="legend">
        <span>
          <span className="swatch" style={{ background: "var(--ink-3)" }} /> Kp below 4
        </span>
        <span>
          <span className="swatch" style={{ background: "var(--caution)" }} /> 4, active
        </span>
        <span>
          <span className="swatch" style={{ background: "var(--alert)" }} /> 5 and over, storm
        </span>
      </div>
      {p.geomagneticRationale && <p className="note">{p.geomagneticRationale}</p>}
      <details>
        <summary>Kp by period, as a table</summary>
        <table className="results compact">
          <thead>
            <tr>
              <th className="left">UTC ({zone.name})</th>
              {p.days.map((day) => (
                <th key={day}>{day}</th>
              ))}
            </tr>
          </thead>
          <tbody>
            {p.kp.map((row) => (
              <tr key={row.period}>
                <th>{period(row.period, zone)}</th>
                {row.values.map((kp, i) => (
                  <td key={i}>{kpText(kp)}</td>
                ))}
              </tr>
            ))}
          </tbody>
        </table>
      </details>
      <table className="results compact">
        <thead>
          <tr>
            <th className="left">Chance of</th>
            {p.days.map((day) => (
              <th key={day}>{day}</th>
            ))}
          </tr>
        </thead>
        <tbody>
          <tr>
            <th>Radio blackout, R1–R2</th>
            {percent(p.blackoutR1R2Pct)}
          </tr>
          <tr>
            <th>Radio blackout, R3 or worse</th>
            {percent(p.blackoutR3Pct)}
          </tr>
          <tr>
            <th>Radiation storm, S1 or worse</th>
            {percent(p.radiationStormPct)}
          </tr>
        </tbody>
      </table>
      {p.blackoutRationale && <p className="note">{p.blackoutRationale}</p>}
    </>
  );
}

function OutlookBody({ p }: { p: Outlook27 }) {
  const largest = Math.max(1, ...p.days.map((d) => d.solarFlux ?? 0));
  return (
    <div className="scroll">
      <table className="results compact">
        <thead>
          <tr>
            <th className="left">Date</th>
            <th>Solar flux</th>
            <th />
            <th>A index</th>
            <th>Largest Kp</th>
          </tr>
        </thead>
        <tbody>
          {p.days.map((day) => (
            <tr key={day.date}>
              <th>{day.date}</th>
              <td className="num">{day.solarFlux ?? "—"}</td>
              <td className="left">
                <span className="meter model">
                  <span className="track" style={{ width: 80 }}>
                    <span className="fill" style={{ width: `${((day.solarFlux ?? 0) / largest) * 100}%` }} />
                  </span>
                </span>
              </td>
              <td className="num">{day.aIndex ?? "—"}</td>
              <td className={day.maxKp !== null && day.maxKp >= STORM_KP ? "num error" : "num"}>{kpText(day.maxKp)}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

function WwvBody({ p }: { p: Wwv }) {
  return (
    <>
      <p>{p.past24h}</p>
      <p>{p.next24h}</p>
    </>
  );
}

function SgasBody({ p }: { p: Sgas }) {
  return (
    <>
      <dl>
        <dt>Indices for</dt>
        <dd>{shown(p.dataDate)}</dd>
        <dt>Solar flux</dt>
        <dd>{shown(p.solarFlux)}</dd>
        <dt>Sunspot number, daily</dt>
        <dd>{shown(p.sunspotNumber)}</dd>
        <dt>A index</dt>
        <dd>
          Fredericksburg {shown(p.aFredericksburg)}, planetary {shown(p.aPlanetary)}
        </dd>
        <dt>X-ray background</dt>
        <dd>{shown(p.xrayBackground)}</dd>
        <dt>Planetary K, 3-hourly</dt>
        <dd className="num">{p.planetaryK.map((k) => k ?? "?").join("  ")}</dd>
        <dt>Flares</dt>
        <dd>{p.energeticEvents.length === 0 ? "None" : p.energeticEvents.join("; ")}</dd>
        <dt>Proton events</dt>
        <dd>{shown(p.protonEvents)}</dd>
      </dl>
      {p.geomagneticSummary && <p>{p.geomagneticSummary}</p>}
    </>
  );
}

function productState(status: ProductStatus | undefined): "current" | "stale" | "none" {
  if (!status?.stored) return "none";
  return status.stale ? "stale" : "current";
}

function provenance(status: ProductStatus | undefined): string {
  if (!status?.stored || status.ageSeconds === null) return "not received";
  return `${TRANSPORT[status.stored.transport]}, ${age(status.ageSeconds)} old${status.stale ? ", stale" : ""}`;
}

function ProductSection({ status }: { status: ProductStatus }) {
  const stored = status.stored;
  return (
    <details className="panel" style={{ marginTop: 10 }}>
      <summary style={{ margin: 0 }}>
        <strong>{status.title}</strong> <Badge state={productState(status)} />
      </summary>
      {stored && status.ageSeconds !== null ? (
        <>
          <p className="note">
            Issued {stampBoth(stored.issued)}, {age(status.ageSeconds)} old. Received by{" "}
            {TRANSPORT[stored.transport]} {stampBoth(stored.received)}.
          </p>
          {stored.product.kind === "wwv" && <WwvBody p={stored.product} />}
          {stored.product.kind === "sgas" && <SgasBody p={stored.product} />}
          {stored.product.kind === "threeDay" && <ThreeDayBody p={stored.product} />}
          {stored.product.kind === "outlook27" && <OutlookBody p={stored.product} />}
        </>
      ) : (
        <p className="note">Not received yet. Source: NOAA SWPC, or Winlink catalog item {status.winlinkId}.</p>
      )}
    </details>
  );
}

function SsnTable({ table, ssn }: { table: SsnTableStatus; ssn: SsnUsed | null }) {
  return (
    <div className="panel" style={{ maxWidth: 860 }}>
      <div className="panel-head">
        <h3>What the model uses</h3>
        <Badge state={table.stale ? "stale" : "current"} />
      </div>
      <p>
        The prediction takes one solar input: the monthly smoothed sunspot number
        {ssn ? (
          <>
            , <span className="num">{ssn.value}</span> for the month shown ({ssn.kind === "manual" ? "entered by hand" : ssn.kind})
          </>
        ) : null}
        . Predictions describe a typical day of the month; the readings above are for your own
        judgement and do not change the predicted numbers.
      </p>
      <dl>
        <dt>Table built</dt>
        <dd>
          {table.generated}, {age(table.ageSeconds)} old, {table.downloaded ? "downloaded from NOAA" : "bundled with the app"}
        </dd>
        <dt>Observed through</dt>
        <dd>{table.lastObservedMonth}</dd>
        <dt>Predicted through</dt>
        <dd>{table.lastPredictedMonth}</dd>
        <dt>Source</dt>
        <dd>{table.source}</dd>
      </dl>
    </div>
  );
}

type Props = { conditions: Conditions; onUpdate: (conditions: Conditions) => void; ssn: SsnUsed | null };
type Result = { ok: boolean; text: string };

/** Solar and geophysical data with its source and age, and the ways to update it. */
export function ConditionsPanel({ conditions, onUpdate, ssn }: Props) {
  const [busy, setBusy] = useState(false);
  const [results, setResults] = useState<Result[]>([]);
  const [request, setRequest] = useState<string | null>(null);
  const [pasted, setPasted] = useState("");

  const find = (kind: string) => conditions.products.find((p) => p.kind === kind);
  const wwvStatus = find("wwv");
  const sgasStatus = find("sgas");
  const threeStatus = find("threeDay");
  const wwv = wwvStatus?.stored?.product.kind === "wwv" ? wwvStatus.stored.product : null;
  const sgas = sgasStatus?.stored?.product.kind === "sgas" ? sgasStatus.stored.product : null;
  const three = threeStatus?.stored?.product.kind === "threeDay" ? threeStatus.stored.product : null;

  async function refresh() {
    setBusy(true);
    try {
      const update = await invoke<ConditionsUpdate<FetchResult>>("refresh_conditions");
      onUpdate(update.conditions);
      setResults(update.results.map((r) => ({ ok: r.ok, text: `${r.title}: ${r.detail}` })));
    } catch (error) {
      setResults([{ ok: false, text: `Could not reach NOAA: ${error}` }]);
    }
    setBusy(false);
  }

  async function importText(text: string, fromFile: boolean, label: string) {
    try {
      const update = await invoke<ConditionsUpdate<Imported>>("import_conditions", { text, fromFile });
      onUpdate(update.conditions);
      return update.results.map((r) => ({
        ok: r.outcome !== "notUnderstood",
        text: `${r.title}: ${OUTCOME[r.outcome]}${r.detail ? ` (${r.detail})` : ""}`,
      }));
    } catch (error) {
      return [{ ok: false, text: `${label}: ${error}` }];
    }
  }

  async function importFiles(event: ChangeEvent<HTMLInputElement>) {
    const files = Array.from(event.target.files ?? []);
    event.target.value = "";
    const all: Result[] = [];
    for (const file of files) all.push(...(await importText(await file.text(), true, file.name)));
    setResults(all);
  }

  return (
    <section className="conditions">
      <div className="view-head">
        <h2>Space weather</h2>
        <span className="hint">Readings for your judgement; the model itself uses only the sunspot table</span>
      </div>

      <div className="controls">
        <button type="button" className="primary" onClick={refresh} disabled={busy}>
          {busy ? "Fetching…" : "Refresh from NOAA"}
        </button>
        <button type="button" onClick={async () => setRequest(request === null ? await invoke<string>("winlink_request") : null)}>
          {request === null ? "Request over Winlink" : "Hide the Winlink request"}
        </button>
        <label className="inline">
          Import files
          <input type="file" multiple onChange={importFiles} />
        </label>
      </div>

      {results.length > 0 && (
        <ul className="results-list">
          {results.map((r, i) => (
            <li key={i} className={r.ok ? undefined : "error"}>
              {r.ok ? "✓" : "✗"} {r.text}
            </li>
          ))}
        </ul>
      )}

      {request !== null && (
        <div className="panel" style={{ maxWidth: 860, marginBottom: 12 }}>
          <p>
            With no Internet, send this as a Winlink message. Each line of the body is one catalog item.
            When the replies arrive, import the message files above or paste them below.
          </p>
          <pre>{request}</pre>
          <button type="button" onClick={() => navigator.clipboard.writeText(request)}>
            Copy the request
          </button>
        </div>
      )}

      <h3>Now</h3>
      <div className="readouts">
        <Reading
          label="Solar flux, 10.7 cm"
          value={wwv?.solarFlux ?? null}
          scale={fluxWords(wwv?.solarFlux ?? null)}
          provenance={provenance(wwvStatus)}
          state={productState(wwvStatus)}
        />
        <Reading
          label="A index, planetary"
          value={wwv?.aIndex ?? null}
          scale={aWords(wwv?.aIndex ?? null)}
          provenance={provenance(wwvStatus)}
          state={productState(wwvStatus)}
        />
        <Reading
          label={`K index${wwv?.kTime ? ` at ${wwv.kTime}` : ""}`}
          value={wwv?.kIndex ?? null}
          scale={kWords(wwv?.kIndex ?? null)}
          provenance={provenance(wwvStatus)}
          state={productState(wwvStatus)}
        />
        <Reading
          label="Sunspot number, daily"
          value={sgas?.sunspotNumber ?? null}
          scale={sgas?.dataDate ? `for ${sgas.dataDate}` : ""}
          provenance={provenance(sgasStatus)}
          state={productState(sgasStatus)}
        />
        <Reading
          label="X-ray background"
          value={sgas?.xrayBackground ?? null}
          scale={sgas ? (sgas.energeticEvents.length === 0 ? "no flares reported" : `${sgas.energeticEvents.length} flares`) : ""}
          provenance={provenance(sgasStatus)}
          state={productState(sgasStatus)}
        />
      </div>

      <h3>Next three days</h3>
      {three ? (
        <ThreeDayBody p={three} />
      ) : (
        <p className="note">No three-day forecast received yet. Refresh from NOAA, or request it over Winlink.</p>
      )}

      <h3>Products</h3>
      <SsnTable table={conditions.ssnTable} ssn={ssn} />
      {conditions.products.map((status) => (
        <ProductSection key={status.kind} status={status} />
      ))}

      <h3>Paste a product</h3>
      <div className="row paste" style={{ maxWidth: 860 }}>
        <textarea
          rows={3}
          placeholder="Paste a NOAA product or a Winlink reply here"
          value={pasted}
          onChange={(e) => setPasted(e.target.value)}
        />
        <button
          type="button"
          disabled={!pasted.trim()}
          onClick={async () => {
            setResults(await importText(pasted, false, "Pasted text"));
            setPasted("");
          }}
        >
          Import pasted text
        </button>
      </div>
    </section>
  );
}
