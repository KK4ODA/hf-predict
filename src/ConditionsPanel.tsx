import { ChangeEvent, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  Conditions,
  ConditionsUpdate,
  FetchResult,
  Imported,
  Outlook27,
  ProductStatus,
  Sgas,
  SsnTableStatus,
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

function utc(unixSeconds: number): string {
  return new Date(unixSeconds * 1000).toISOString().slice(0, 16).replace("T", " ") + " UTC";
}

function age(seconds: number): string {
  if (seconds < 90 * 60) return `${Math.max(0, Math.round(seconds / 60))} min`;
  if (seconds < 48 * 3600) return `${Math.round(seconds / 3600)} h`;
  return `${Math.round(seconds / 86400)} days`;
}

const shown = (value: number | string | null) => (value === null ? "not reported" : String(value));

function Badge({ state }: { state: "current" | "stale" | "none" }) {
  const text = { current: "● Current", stale: "◐ Stale", none: "– No data" }[state];
  return <span className={`badge badge-${state}`}>{text}</span>;
}

/** Cell shading for a Kp value: one hue, stronger with more. */
function kpShade(kp: number | null): string {
  return `rgba(57, 135, 229, ${(((kp ?? 0) / 9) * 0.7).toFixed(2)})`;
}

function kpText(kp: number | null): string {
  if (kp === null) return "—";
  return kp >= STORM_KP ? `${kp} (G${Math.min(5, Math.floor(kp) - 4)})` : String(kp);
}

function WwvBody({ p }: { p: Wwv }) {
  return (
    <>
      <dl>
        <dt>Solar flux</dt>
        <dd>{shown(p.solarFlux)}</dd>
        <dt>A index</dt>
        <dd>{shown(p.aIndex)}</dd>
        <dt>K index</dt>
        <dd>
          {shown(p.kIndex)}
          {p.kTime && ` at ${p.kTime}`}
        </dd>
      </dl>
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
        <dt>Sunspot number (daily)</dt>
        <dd>{shown(p.sunspotNumber)}</dd>
        <dt>A index</dt>
        <dd>
          Fredericksburg {shown(p.aFredericksburg)}, planetary {shown(p.aPlanetary)}
        </dd>
        <dt>X-ray background</dt>
        <dd>{shown(p.xrayBackground)}</dd>
        <dt>Planetary K, 3-hourly</dt>
        <dd>{p.planetaryK.map((k) => k ?? "?").join("  ")}</dd>
        <dt>Flares</dt>
        <dd>{p.energeticEvents.length === 0 ? "None" : p.energeticEvents.join("; ")}</dd>
        <dt>Proton events</dt>
        <dd>{shown(p.protonEvents)}</dd>
      </dl>
      {p.geomagneticSummary && <p>{p.geomagneticSummary}</p>}
    </>
  );
}

function ThreeDayBody({ p }: { p: ThreeDay }) {
  const percent = (values: (number | null)[]) =>
    values.map((v, i) => <td key={i}>{v === null ? "—" : `${v}%`}</td>);
  return (
    <>
      <table className="results compact">
        <thead>
          <tr>
            <th>Kp, UTC</th>
            {p.days.map((day) => (
              <th key={day}>{day}</th>
            ))}
          </tr>
        </thead>
        <tbody>
          {p.kp.map((row) => (
            <tr key={row.period}>
              <th>{row.period.replace("UT", "")}</th>
              {row.values.map((kp, i) => (
                <td key={i} style={{ background: kpShade(kp) }}>
                  {kpText(kp)}
                </td>
              ))}
            </tr>
          ))}
        </tbody>
      </table>
      {p.geomagneticRationale && <p>{p.geomagneticRationale}</p>}
      <table className="results compact">
        <thead>
          <tr>
            <th>Chance of</th>
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
      {p.blackoutRationale && <p>{p.blackoutRationale}</p>}
    </>
  );
}

function OutlookBody({ p }: { p: Outlook27 }) {
  return (
    <div className="scroll">
      <table className="results compact">
        <thead>
          <tr>
            <th>Date</th>
            <th>Solar flux</th>
            <th>A index</th>
            <th>Largest Kp</th>
          </tr>
        </thead>
        <tbody>
          {p.days.map((day) => (
            <tr key={day.date}>
              <th>{day.date}</th>
              <td>{day.solarFlux ?? "—"}</td>
              <td>{day.aIndex ?? "—"}</td>
              <td style={{ background: kpShade(day.maxKp) }}>{kpText(day.maxKp)}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

function ProductCard({ status }: { status: ProductStatus }) {
  const stored = status.stored;
  return (
    <article className="card">
      <header>
        <h3>{status.title}</h3>
        <Badge state={!stored ? "none" : status.stale ? "stale" : "current"} />
      </header>
      {stored && status.ageSeconds !== null ? (
        <>
          <p className="provenance">
            Issued {utc(stored.issued)} · {age(status.ageSeconds)} old · received by{" "}
            {TRANSPORT[stored.transport]} {utc(stored.received)}
          </p>
          {stored.product.kind === "wwv" && <WwvBody p={stored.product} />}
          {stored.product.kind === "sgas" && <SgasBody p={stored.product} />}
          {stored.product.kind === "threeDay" && <ThreeDayBody p={stored.product} />}
          {stored.product.kind === "outlook27" && <OutlookBody p={stored.product} />}
        </>
      ) : (
        <p className="provenance">
          Not received yet. Source: NOAA SWPC, or Winlink catalog item {status.winlinkId}.
        </p>
      )}
    </article>
  );
}

function SsnTableCard({ table }: { table: SsnTableStatus }) {
  return (
    <article className="card">
      <header>
        <h3>Smoothed sunspot table</h3>
        <Badge state={table.stale ? "stale" : "current"} />
      </header>
      <p className="provenance">
        Built {table.generated} · {age(table.ageSeconds)} old ·{" "}
        {table.downloaded ? "downloaded from NOAA" : "bundled with the app"}
      </p>
      <dl>
        <dt>Observed through</dt>
        <dd>{table.lastObservedMonth}</dd>
        <dt>Predicted through</dt>
        <dd>{table.lastPredictedMonth}</dd>
        <dt>Source</dt>
        <dd>{table.source}</dd>
      </dl>
      <p>
        This is the only solar input the prediction model takes. Predictions are climatological:
        they describe a typical day of the month. The indices in the other cards are for your own
        judgement and do not change the predicted numbers.
      </p>
    </article>
  );
}

type Props = { conditions: Conditions; onUpdate: (conditions: Conditions) => void };
type Result = { ok: boolean; text: string };

/** Solar and geophysical data with its source and age, and the ways to update it. */
export function ConditionsPanel({ conditions, onUpdate }: Props) {
  const [busy, setBusy] = useState(false);
  const [results, setResults] = useState<Result[]>([]);
  const [request, setRequest] = useState<string | null>(null);
  const [pasted, setPasted] = useState("");

  async function refresh() {
    setBusy(true);
    try {
      const update = await invoke<ConditionsUpdate<FetchResult>>("refresh_conditions");
      onUpdate(update.conditions);
      setResults(update.results.map((r) => ({ ok: r.ok, text: `${r.title}: ${r.detail}` })));
    } catch (error) {
      setResults([{ ok: false, text: String(error) }]);
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
      <div className="controls">
        <button type="button" onClick={refresh} disabled={busy}>
          {busy ? "Fetching…" : "Refresh from NOAA"}
        </button>
        <button
          type="button"
          onClick={async () => setRequest(request === null ? await invoke<string>("winlink_request") : null)}
        >
          Winlink request
        </button>
        <label className="file">
          Import files
          <input type="file" multiple onChange={importFiles} />
        </label>
      </div>

      {request !== null && (
        <div className="card">
          <p>
            With no Internet, send this as a Winlink message. Each line of the body is one catalog
            item. When the replies arrive, import the message files above or paste them below.
          </p>
          <pre>{request}</pre>
          <button type="button" onClick={() => navigator.clipboard.writeText(request)}>
            Copy
          </button>
        </div>
      )}

      <div className="row paste">
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

      {results.length > 0 && (
        <ul className="results-list">
          {results.map((r, i) => (
            <li key={i} className={r.ok ? undefined : "error"}>
              {r.ok ? "✓" : "✗"} {r.text}
            </li>
          ))}
        </ul>
      )}

      <div className="cards">
        <SsnTableCard table={conditions.ssnTable} />
        {conditions.products.map((status) => (
          <ProductCard key={status.kind} status={status} />
        ))}
      </div>
    </section>
  );
}
