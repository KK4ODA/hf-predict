import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { FoundProgram, ListenerStatus, RadioConfig, RadioStatus, RigModel } from "./types";

const POLL_MS = 2000;
const BAUDS = [4800, 9600, 19200, 38400, 57600, 115200];

const mhz = (hz: number) => (hz / 1e6).toFixed(3);

function age(seconds: number): string {
  return seconds < 90 ? `${Math.max(0, Math.round(seconds))} s ago` : `${Math.round(seconds / 60)} min ago`;
}

/** What the radio reports through rigctld, and whether it agrees with WSJT-X. */
export function RadioPanel() {
  const [status, setStatus] = useState<RadioStatus | null>(null);
  const [draft, setDraft] = useState<RadioConfig | null>(null);
  const [wsjtx, setWsjtx] = useState<ListenerStatus | null>(null);
  const [programs, setPrograms] = useState<FoundProgram[]>([]);
  const [wsjtxPrograms, setWsjtxPrograms] = useState<FoundProgram[]>([]);
  const [launch, setLaunch] = useState<string | null>(null);
  const [ports, setPorts] = useState<string[]>([]);
  const [models, setModels] = useState<RigModel[]>([]);
  const [error, setError] = useState("");
  const [now, setNow] = useState(Date.now() / 1000);

  useEffect(() => {
    let current = true;
    async function poll() {
      try {
        const [radio, listener, launched] = await Promise.all([
          invoke<RadioStatus>("radio_status"),
          invoke<ListenerStatus>("listener_status"),
          invoke<string | null>("wsjtx_launch"),
        ]);
        if (!current) return;
        setStatus(radio);
        setDraft((existing) => existing ?? radio.config);
        setWsjtx(listener);
        setLaunch(launched);
        setNow(Date.now() / 1000);
        setError("");
      } catch (e) {
        if (current) setError(String(e));
      }
    }
    poll();
    const timer = setInterval(poll, POLL_MS);
    invoke<FoundProgram[]>("find_rigctld").then((found) => current && setPrograms(found)).catch(() => {});
    invoke<string[]>("serial_ports").then((found) => current && setPorts(found)).catch(() => {});
    invoke<FoundProgram[]>("find_wsjtx").then((found) => current && setWsjtxPrograms(found)).catch(() => {});
    return () => {
      current = false;
      clearInterval(timer);
    };
  }, []);

  // The radio models the chosen program knows.
  const program = draft?.rigctldPath.trim() ?? "";
  useEffect(() => {
    if (program === "") {
      setModels([]);
      return;
    }
    let current = true;
    invoke<RigModel[]>("rig_models", { program })
      .then((list) => current && setModels(list))
      .catch(() => current && setModels([]));
    return () => {
      current = false;
    };
  }, [program]);

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
  const daemon = status.daemon;
  const decoder = wsjtx?.tracker?.decoders[0] ?? null;
  const agreement =
    radio && decoder?.dialHz != null
      ? radio.freqHz === decoder.dialHz
        ? "WSJT-X reports the same dial frequency, so both are seeing the same radio."
        : `WSJT-X reports ${mhz(decoder.dialHz)} MHz: it may be on another VFO, or not using this rigctld.`
      : null;
  const chosenModel = models.find((m) => m.number === draft.rigModel);

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
      </div>
      <div className="controls">
        <label className="inline">
          <input
            type="checkbox"
            checked={draft.startRigctld}
            onChange={(e) => setDraft({ ...draft, startRigctld: e.target.checked })}
          />
          Start rigctld for me
        </label>
        <label className="inline">
          Program
          <input
            className="wide"
            list="rigctld-programs"
            placeholder="path to rigctld"
            value={draft.rigctldPath}
            onChange={(e) => setDraft({ ...draft, rigctldPath: e.target.value })}
          />
          <datalist id="rigctld-programs">
            {programs.map((p) => (
              <option key={p.path} value={p.path}>
                {p.version}
              </option>
            ))}
          </datalist>
        </label>
      </div>
      <div className="controls">
        <label className="inline">
          Radio
          <select
            value={draft.rigModel}
            onChange={(e) => setDraft({ ...draft, rigModel: Number(e.target.value) })}
            disabled={models.length === 0}
          >
            <option value={0}>{models.length === 0 ? (program === "" ? "choose a program first" : "no model list") : "choose…"}</option>
            {models.map((m) => (
              <option key={m.number} value={m.number}>
                {m.maker} {m.model} ({m.number})
              </option>
            ))}
          </select>
        </label>
        <label className="inline">
          Serial port
          <input
            className="short"
            list="serial-ports"
            placeholder="COM6"
            value={draft.serialPort}
            onChange={(e) => setDraft({ ...draft, serialPort: e.target.value })}
          />
          <datalist id="serial-ports">
            {ports.map((p) => (
              <option key={p} value={p} />
            ))}
          </datalist>
        </label>
        <label className="inline">
          Baud
          <select value={draft.baud} onChange={(e) => setDraft({ ...draft, baud: Number(e.target.value) })}>
            {BAUDS.map((b) => (
              <option key={b} value={b}>
                {b}
              </option>
            ))}
          </select>
        </label>
        <button type="button" onClick={apply}>
          Apply
        </button>
      </div>
      <div className="controls">
        <label className="inline">
          <input
            type="checkbox"
            checked={draft.startWsjtx}
            onChange={(e) => setDraft({ ...draft, startWsjtx: e.target.checked })}
          />
          Start WSJT-X once rigctld is up
        </label>
        <label className="inline">
          Program
          <input
            className="wide"
            list="wsjtx-programs"
            placeholder="path to wsjtx or ws"
            value={draft.wsjtxPath}
            onChange={(e) => setDraft({ ...draft, wsjtxPath: e.target.value })}
          />
          <datalist id="wsjtx-programs">
            {wsjtxPrograms.map((p) => (
              <option key={p.path} value={p.path}>
                {p.version}
              </option>
            ))}
          </datalist>
        </label>
      </div>
      {launch && draft.startWsjtx && <p className="hint">{launch}</p>}
      {programs.length === 0 && draft.startRigctld && (
        <p className="note">
          No rigctld found in the usual places. Install Hamlib (it comes with WSJT-X on some
          platforms as rigctld-wsjtx) and enter the path to its rigctld program.
        </p>
      )}

      <p className={status.state === "failed" ? "error" : undefined}>{status.detail}</p>
      {status.lastError && status.state !== "failed" && <p className="error">Last problem: {status.lastError}</p>}
      {daemon && (
        <p className="hint">
          {daemon.running ? `rigctld started by this app (process ${daemon.pid})` : `rigctld exited with code ${daemon.exitCode ?? "?"}`}
          {chosenModel && ` for ${chosenModel.maker} ${chosenModel.model}`}: <code>{daemon.command}</code>
          {daemon.output && !daemon.running && <pre className="output">{daemon.output}</pre>}
        </p>
      )}

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
          {!decoder && status.state === "connected" && (
            <p className="hint">WSJT-X is not reporting over UDP, so its dial frequency cannot be compared.</p>
          )}
        </article>
      )}

      <h3>Setting up</h3>
      <p className="note">
        This app never transmits and, in this version, never changes the radio: it only reads
        frequency, mode, PTT, split and VFO. It shares the radio with WSJT-X through Hamlib's{" "}
        <code>rigctld</code>. Tick <em>Start rigctld for me</em>, choose the program, your radio,
        its serial port and speed, and Apply: the app starts the daemon, bound to this computer,
        and stops it when the app closes. Then in WSJT-X set the rig to <em>Hamlib NET rigctl</em>{" "}
        with network server <code>127.0.0.1:4532</code> and the same PTT method as before. WSJT-X
        only looks for the daemon when it starts, so either start this app first, press Retry in
        WSJT-X's rig error once the daemon is up, or tick <em>Start WSJT-X once rigctld is up</em>{" "}
        and let this app start it in the right order. Both
        programs then talk to the same daemon; reading every couple of seconds is enough, since
        rigctld answers from a one-second cache. If a rigctld is already running, leave the
        start box clear and the app attaches to it.
      </p>
    </section>
  );
}
