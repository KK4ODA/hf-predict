import { useState } from "react";
import { relaunch } from "@tauri-apps/plugin-process";
import { check, Update } from "@tauri-apps/plugin-updater";

type UpdateState =
  | { kind: "idle" }
  | { kind: "checking" }
  | { kind: "current" }
  | { kind: "available"; update: Update }
  | { kind: "installing"; version: string }
  | { kind: "failed"; error: string };

export function UpdateCheck() {
  const [state, setState] = useState<UpdateState>({ kind: "idle" });

  async function checkForUpdate() {
    setState({ kind: "checking" });
    try {
      const found = await check();
      setState(found ? { kind: "available", update: found } : { kind: "current" });
    } catch (error) {
      setState({ kind: "failed", error: String(error) });
    }
  }

  async function install(found: Update) {
    setState({ kind: "installing", version: found.version });
    try {
      await found.downloadAndInstall();
      await relaunch();
    } catch (error) {
      setState({ kind: "failed", error: String(error) });
    }
  }

  return (
    <span className="update">
      <button
        type="button"
        onClick={checkForUpdate}
        disabled={state.kind === "checking" || state.kind === "installing"}
      >
        Check for updates
      </button>
      {state.kind === "checking" && " Checking…"}
      {state.kind === "current" && <span className="ok"> This is the latest version.</span>}
      {state.kind === "available" && (
        <>
          {" "}
          Version {state.update.version} is available.{" "}
          <button type="button" onClick={() => install(state.update)}>
            Install and restart
          </button>
        </>
      )}
      {state.kind === "installing" && ` Installing version ${state.version}…`}
      {state.kind === "failed" && (
        <span className="error"> Could not check for updates: {state.error}</span>
      )}
    </span>
  );
}
