import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { relaunch } from "@tauri-apps/plugin-process";
import { check, Update } from "@tauri-apps/plugin-updater";

export type UpdateState =
  | { kind: "idle" }
  | { kind: "checking" }
  | { kind: "current" }
  | { kind: "available"; update: Update }
  | { kind: "downloading"; version: string; done: number; total: number | null }
  | { kind: "installing"; version: string }
  | { kind: "failed"; error: string; during: "check" | "install" };

/** The first look happens a little after start-up, then every six hours. */
const FIRST_CHECK_MS = 5000;
const RECHECK_MS = 6 * 3600 * 1000;

export type Updater = {
  state: UpdateState;
  checkNow: () => void;
  install: (update: Update) => void;
};

/**
 * Looks for a newer release by itself. A failed background check (no
 * network, say) is silent; one the operator asked for says what went wrong.
 */
export function useUpdater(): Updater {
  const [state, setState] = useState<UpdateState>({ kind: "idle" });

  const look = useCallback(async (quiet: boolean) => {
    if (!quiet) setState({ kind: "checking" });
    try {
      const found = await check();
      setState((current) =>
        current.kind === "downloading" || current.kind === "installing"
          ? current
          : found
            ? { kind: "available", update: found }
            : { kind: "current" },
      );
    } catch (error) {
      if (!quiet) setState({ kind: "failed", error: String(error), during: "check" });
    }
  }, []);

  useEffect(() => {
    const first = setTimeout(() => look(true), FIRST_CHECK_MS);
    const timer = setInterval(() => look(true), RECHECK_MS);
    return () => {
      clearTimeout(first);
      clearInterval(timer);
    };
  }, [look]);

  const install = useCallback(async (update: Update) => {
    setState({ kind: "downloading", version: update.version, done: 0, total: null });
    let done = 0;
    try {
      await update.download((event) => {
        if (event.event === "Started") {
          setState({ kind: "downloading", version: update.version, done: 0, total: event.data.contentLength ?? null });
        } else if (event.event === "Progress") {
          done += event.data.chunkLength;
          setState((s) => (s.kind === "downloading" ? { ...s, done } : s));
        }
      });
    } catch (error) {
      setState({ kind: "failed", error: `The download failed: ${error}`, during: "install" });
      return;
    }
    setState({ kind: "installing", version: update.version });
    // Stop any scan, which puts the radio back, and leave rigctld running
    // for the new version to take back. On Windows the installer closes the
    // app itself, so this happens first.
    await invoke("prepare_for_restart").catch(() => {});
    try {
      await update.install();
      await relaunch();
    } catch (error) {
      await invoke("cancel_restart").catch(() => {});
      setState({ kind: "failed", error: `The update could not be installed: ${error}`, during: "install" });
    }
  }, []);

  return { state, checkNow: () => look(false), install };
}

const megabytes = (bytes: number) => (bytes / 1048576).toFixed(1);

/** One line on the state of an update, for the notice under the path bar. */
export function updateNoticeText(state: UpdateState): string | null {
  switch (state.kind) {
    case "available":
      return `Version ${state.update.version} is ready to install. Any scan stops and the radio goes back first; the app then restarts.`;
    case "downloading":
      return `Downloading ${state.version}: ${megabytes(state.done)}${state.total !== null ? ` of ${megabytes(state.total)}` : ""} MB…`;
    case "installing":
      return `Installing ${state.version}; the app will restart.`;
    case "failed":
      return state.during === "install" ? state.error : null;
    default:
      return null;
  }
}

/** The update controls on the Stations view. */
export function UpdateCheck({ updater }: { updater: Updater }) {
  const { state, checkNow, install } = updater;
  const busy = state.kind === "checking" || state.kind === "downloading" || state.kind === "installing";
  return (
    <span className="update">
      {state.kind === "available" ? (
        <button type="button" className="primary" onClick={() => install(state.update)}>
          Install {state.update.version} and restart
        </button>
      ) : (
        <button type="button" onClick={checkNow} disabled={busy}>
          Check for updates
        </button>
      )}
      {state.kind === "checking" && <span className="hint">Checking…</span>}
      {state.kind === "current" && <span className="hint">This is the latest version.</span>}
      {state.kind === "available" && <span className="hint">Version {state.update.version} is available.</span>}
      {state.kind === "downloading" && (
        <span className="hint">
          Downloading {state.version}: {megabytes(state.done)}
          {state.total !== null && ` of ${megabytes(state.total)}`} MB
        </span>
      )}
      {state.kind === "installing" && <span className="hint">Installing {state.version}; the app will restart.</span>}
      {state.kind === "failed" && <span className="error">{state.error}</span>}
    </span>
  );
}
