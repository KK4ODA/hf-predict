import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { ListenerStatus, RadioConfig, RadioStatus } from "./types";

const POLL_MS = 2000;

const mhz = (hz: number) => (hz / 1e6).toFixed(3);

function age(seconds: number): string {
  return seconds < 90 ? `${Math.max(0, Math.round(seconds))} s ago` : `${Math.round(seconds / 60)} min ago`;
}

/** What the radio reports through rigctld, and whether it agrees with WSJT-X. */
export function RadioPanel() {
  const [status, setStatus] = useState<RadioStatus | null>(null);
  const [draft, setDraft] = useState<RadioConfig | null>(null);
  const [wsjtx, setWsjtx] = useState<ListenerStatus | null>(null);
  const [error, setError] = useState("");
  const [now, setNow] = useState(Date.now() / 1000);

  useEffect(() => {
    let current = true;
    async function poll() {
      try {
        const [radio, listener] = await Promise.all([
          invoke<RadioStatus>("radio_status"),
          invoke<ListenerStatus>("listener_status"),
        ]);
        if (!current) return;
        setStatus(radio);
        setDraft((existing) => existing ?? radio.config);
        setWsjtx(listener);
        setNow(Date.now() / 1000);
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
  }, []);

  async function apply() {
    if (!draft) return;
    try {
      setStatus(await invoke<RadioStatus>("set_radio_config", { config: draft }));
      setError("");
    } catch (e) {
      setError(String(e));
    }
  }

  if (!status || !draft) return <p className="note">{error || "Loading…"}</p>;
  const radio = status.radio;
  const decoder = wsjtx?.tracker?.decoders[0] ?? null;
  const agreement =
    radio && decoder?.dialHz != null
      ? radio.freqHz === decoder.dialHz
        ? "WSJT-X reports the same dial frequency, so both are seeing the same radio."
        : `WSJT-X reports ${mhz(decoder.dialHz)} MHz: it may be on another VFO, or not using this rigctld.`
      : null;

  return (
    <section>
      {error && <p className="error">{error}</p>}
      <div className="controls">
        <label className="inline">
          <input
            type="checkbox"
            checked={draft.enabled}
            onChange={(e) => setDraft({ ...draft, enabled: e.target.checked })}
          />
          Read the radio through rigctld
        </label>
        <label className="inline">
          Host
          <input className="short" value={draft.host} onChange={(e) => setDraft({ ...draft, host: e.target.value })} />
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
        <label className="inline">
          Every
          <input
            className="short"
            type="number"
            min={0.5}
            step={0.5}
            value={draft.pollSeconds}
            onChange={(e) => setDraft({ ...draft, pollSeconds: Number(e.target.value) })}
          />
          s
        </label>
        <button type="button" onClick={apply}>
          Apply
        </button>
      </div>

      <p className={status.state === "failed" ? "error" : undefined}>{status.detail}</p>
      {status.lastError && status.state !== "failed" && <p className="error">Last problem: {status.lastError}</p>}

      {radio && status.readUtc !== null && (
        <article className="card">
          <p className="plan-now">
            <strong>{mhz(radio.freqHz)} MHz</strong> · {radio.band} · {radio.mode}
            {radio.passbandHz !== null && ` ${radio.passbandHz} Hz`} ·{" "}
            {radio.ptt ? <span className="caution">TRANSMITTING</span> : "receiving"}
          </p>
          <p className="hint">
            {radio.vfo && `${radio.vfo} · `}
            split {radio.split === null ? "unknown" : radio.split ? `on, transmit on ${radio.txVfo ?? "?"}` : "off"} · read{" "}
            {age(now - status.readUtc)} · {status.reads} reads, {status.errors} errors
          </p>
          {agreement && <p>{agreement}</p>}
          {radio && !decoder && status.state === "connected" && (
            <p className="hint">WSJT-X is not reporting over UDP, so its dial frequency cannot be compared.</p>
          )}
        </article>
      )}

      <h3>Setting up</h3>
      <p className="note">
        This app never transmits and, in this version, never changes the radio: it only reads
        frequency, mode, PTT, split and VFO. It shares the radio with WSJT-X through Hamlib's{" "}
        <code>rigctld</code>: start <code>rigctld-wsjtx</code> (installed with WSJT-X) for your
        radio, for example{" "}
        <code>rigctld-wsjtx -m &lt;model&gt; -r &lt;serial port&gt; -s &lt;baud&gt; -T 127.0.0.1 -t 4532</code>{" "}
        (<code>rigctld-wsjtx -l</code> lists the model numbers), then in WSJT-X set the rig to{" "}
        <em>Hamlib NET rigctl</em> with network server <code>127.0.0.1:4532</code>. Both programs
        then talk to the same daemon. Reading every couple of seconds is enough; rigctld answers
        from a one-second cache.
      </p>
    </section>
  );
}
