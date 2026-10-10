import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { FoundProgram, ListenerStatus, RadioConfig, RadioStatus, RigModel } from "./types";
import { Health, Pill } from "./ui";

const POLL_MS = 2000;
const BAUDS = [4800, 9600, 19200, 38400, 57600, 115200];

const mhz = (hz: number) => (hz / 1e6).toFixed(3);

function age(seconds: number): string {
  return seconds < 90 ? `${Math.max(0, Math.round(seconds))} s` : `${Math.round(seconds / 60)} min`;
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

  const health: Health =
    status.state === "connected" ? "ok" : status.state === "connecting" ? "warn" : status.state === "failed" ? "alert" : "off";
  const healthLabel = { connected: "Connected", connecting: "Connecting", failed: "Not connected", off: "Off" }[status.state];

  return (
    <section>
      <div className="view-head">
        <h2>Radio</h2>
        <Pill state={health}>{healthLabel}</Pill>
        <span className="hint">Reads the radio, and sets its frequency only while scanning. It never transmits.</span>
      </div>
      {error && <p className="error">{error}</p>}

      {radio && status.readUtc !== null && (
        <div className="panel" style={{ maxWidth: 860, marginBottom: 16 }}>
          <div className="readout">
            <span className="freq">
              {mhz(radio.freqHz)}
              <small>MHz</small>
            </span>
            <span>
              <span className="band-name">{radio.band}</span> {radio.mode}
              {radio.passbandHz !== null && <span className="hint"> {radio.passbandHz} Hz</span>}
            </span>
            {radio.ptt ? <Pill state="alert">Transmitting</Pill> : <Pill state="ok">Receiving</Pill>}
          </div>
          <dl>
            <dt>VFO</dt>
            <dd>{radio.vfo ?? "not reported"}</dd>
            <dt>Split</dt>
            <dd>{radio.split === null ? "not reported" : radio.split ? `on, transmitting on ${radio.txVfo ?? "?"}` : "off"}</dd>
            <dt>Last read</dt>
            <dd>
              {age(now - status.readUtc)} ago, {status.reads} reads, {status.errors} errors
            </dd>
            <dt>WSJT-X</dt>
            <dd>{agreement ?? (status.state === "connected" ? "not reporting over UDP, so its dial cannot be compared" : "—")}</dd>
          </dl>
        </div>
      )}

      <h3>Settings</h3>
      <div className="panel settings">
        <div className="settings-group">
          <label className="inline">
            <input type="checkbox" checked={draft.enabled} onChange={(e) => setDraft({ ...draft, enabled: e.target.checked })} />
            Read the radio through rigctld
          </label>
          <div className="form-grid three">
            <label>
              Host
              <input value={draft.host} onChange={(e) => setDraft({ ...draft, host: e.target.value })} />
            </label>
            <label>
              Port
              <input
                type="number"
                min={1}
                max={65535}
                value={draft.port}
                onChange={(e) => setDraft({ ...draft, port: Number(e.target.value) })}
              />
            </label>
            <label>
              Read every, seconds
              <input
                type="number"
                min={0.5}
                step={0.5}
                value={draft.pollSeconds}
                onChange={(e) => setDraft({ ...draft, pollSeconds: Number(e.target.value) })}
              />
            </label>
          </div>
        </div>

        <div className="settings-group">
          <label className="inline">
            <input type="checkbox" checked={draft.startRigctld} onChange={(e) => setDraft({ ...draft, startRigctld: e.target.checked })} />
            Start rigctld for me
          </label>
          <div className="form-grid">
            <label className="wide-field">
              rigctld program
              <input
                list="rigctld-programs"
                placeholder="path to rigctld"
                value={draft.rigctldPath}
                disabled={!draft.startRigctld}
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
            <label className="span-2">
              Radio
              <select
                value={draft.rigModel}
                onChange={(e) => setDraft({ ...draft, rigModel: Number(e.target.value) })}
                disabled={!draft.startRigctld || models.length === 0}
              >
                <option value={0}>{models.length === 0 ? (program === "" ? "choose a program first" : "no model list") : "choose…"}</option>
                {models.map((m) => (
                  <option key={m.number} value={m.number}>
                    {m.maker} {m.model} ({m.number})
                  </option>
                ))}
              </select>
            </label>
            <label>
              Serial port
              <input
                list="serial-ports"
                placeholder="COM6"
                value={draft.serialPort}
                disabled={!draft.startRigctld}
                onChange={(e) => setDraft({ ...draft, serialPort: e.target.value })}
              />
              <datalist id="serial-ports">
                {ports.map((p) => (
                  <option key={p} value={p} />
                ))}
              </datalist>
            </label>
            <label>
              Speed, baud
              <select value={draft.baud} disabled={!draft.startRigctld} onChange={(e) => setDraft({ ...draft, baud: Number(e.target.value) })}>
                {BAUDS.map((b) => (
                  <option key={b} value={b}>
                    {b}
                  </option>
                ))}
              </select>
            </label>
          </div>
          {programs.length === 0 && draft.startRigctld && (
            <p className="note">
              No rigctld found in the usual places. Install Hamlib (it comes with WSJT-X on some platforms as
              rigctld-wsjtx) and enter the path to its rigctld program.
            </p>
          )}
        </div>

        <div className="settings-group">
          <label className="inline">
            <input type="checkbox" checked={draft.startWsjtx} onChange={(e) => setDraft({ ...draft, startWsjtx: e.target.checked })} />
            Start WSJT-X once rigctld is up
          </label>
          <div className="form-grid">
            <label className="wide-field">
              WSJT-X program
              <input
                list="wsjtx-programs"
                placeholder="path to wsjtx or ws"
                value={draft.wsjtxPath}
                disabled={!draft.startWsjtx}
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
        </div>

        <div className="settings-actions">
          <button type="button" className="primary" onClick={apply}>
            Apply
          </button>
        </div>
      </div>

      <p className={status.state === "failed" ? "error" : undefined}>{status.detail}</p>
      {status.lastError && status.state !== "failed" && <p className="error">Last problem: {status.lastError}</p>}
      {daemon && (
        <p className="hint">
          {daemon.running ? `rigctld started by this app, process ${daemon.pid}` : `rigctld exited with code ${daemon.exitCode ?? "?"}`}
          {chosenModel && ` for ${chosenModel.maker} ${chosenModel.model}`}: <code>{daemon.command}</code>
          {daemon.output && !daemon.running && <pre className="output">{daemon.output}</pre>}
        </p>
      )}

      <h3>Setting up</h3>
      <p className="note">
        This app never transmits. It reads frequency, mode, PTT, split and VFO, and changes only
        the frequency, and only while you run a scan from the Plan view. It shares the radio with WSJT-X through Hamlib's{" "}
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
