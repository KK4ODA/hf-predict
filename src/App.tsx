import { useEffect, useState } from "react";
import { getVersion } from "@tauri-apps/api/app";
import { invoke } from "@tauri-apps/api/core";
import { relaunch } from "@tauri-apps/plugin-process";
import { check, Update } from "@tauri-apps/plugin-updater";
import "./App.css";

type EngineSelfTest = { banner: string; firstHour: string };

type EngineState =
  | { kind: "idle" }
  | { kind: "running" }
  | { kind: "ok"; result: EngineSelfTest }
  | { kind: "failed"; error: string };

type UpdateState =
  | { kind: "idle" }
  | { kind: "checking" }
  | { kind: "current" }
  | { kind: "available"; update: Update }
  | { kind: "installing"; version: string }
  | { kind: "failed"; error: string };

function App() {
  const [version, setVersion] = useState("");
  const [engine, setEngine] = useState<EngineState>({ kind: "idle" });
  const [update, setUpdate] = useState<UpdateState>({ kind: "idle" });

  useEffect(() => {
    getVersion().then(setVersion);
  }, []);

  async function runSelfTest() {
    setEngine({ kind: "running" });
    try {
      setEngine({ kind: "ok", result: await invoke<EngineSelfTest>("engine_self_test") });
    } catch (error) {
      setEngine({ kind: "failed", error: String(error) });
    }
  }

  async function checkForUpdate() {
    setUpdate({ kind: "checking" });
    try {
      const found = await check();
      setUpdate(found ? { kind: "available", update: found } : { kind: "current" });
    } catch (error) {
      setUpdate({ kind: "failed", error: String(error) });
    }
  }

  async function installUpdate(found: Update) {
    setUpdate({ kind: "installing", version: found.version });
    try {
      await found.downloadAndInstall();
      await relaunch();
    } catch (error) {
      setUpdate({ kind: "failed", error: String(error) });
    }
  }

  return (
    <main>
      <h1>
        hf-predict <span className="version">{version}</span>
      </h1>

      <section>
        <h2>Propagation engine</h2>
        <button onClick={runSelfTest} disabled={engine.kind === "running"}>
          Run engine self-test
        </button>
        {engine.kind === "running" && <p>Running…</p>}
        {engine.kind === "failed" && <p className="error">Engine failed: {engine.error}</p>}
        {engine.kind === "ok" && (
          <>
            <p className="ok">Engine ran: {engine.result.banner}</p>
            <pre>{engine.result.firstHour}</pre>
          </>
        )}
      </section>

      <section>
        <h2>Updates</h2>
        <button
          onClick={checkForUpdate}
          disabled={update.kind === "checking" || update.kind === "installing"}
        >
          Check for updates
        </button>
        {update.kind === "checking" && <p>Checking…</p>}
        {update.kind === "current" && <p className="ok">This is the latest version.</p>}
        {update.kind === "available" && (
          <p>
            Version {update.update.version} is available.{" "}
            <button onClick={() => installUpdate(update.update)}>Install and restart</button>
          </p>
        )}
        {update.kind === "installing" && <p>Installing version {update.version}…</p>}
        {update.kind === "failed" && (
          <p className="error">Could not check for updates: {update.error}</p>
        )}
      </section>
    </main>
  );
}

export default App;
