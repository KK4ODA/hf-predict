import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { stampBoth } from "./localtime";
import { FoundLog, LogCheck, LogFile } from "./types";

type Props = {
  files: LogFile[];
  onChange: (files: LogFile[]) => void;
  /** Used as the receiver position of a newly added log. */
  defaultRxPosition: string;
};

function size(bytes: number): string {
  return bytes < 1024 * 1024 ? `${Math.round(bytes / 1024)} kB` : `${(bytes / 1048576).toFixed(1)} MB`;
}

/** The ALL.TXT logs this station's WSJT-X installations keep, and what reading them found. */
export function LogFiles({ files, onChange, defaultRxPosition }: Props) {
  const [checks, setChecks] = useState<LogCheck[]>([]);
  const [found, setFound] = useState<FoundLog[] | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");

  useEffect(() => {
    invoke<LogCheck[]>("log_checks").then(setChecks).catch(() => {});
  }, []);

  const check = async (next: LogFile[]) => {
    setBusy(true);
    setError("");
    try {
      setChecks(await invoke<LogCheck[]>("check_log_files", { files: next }));
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  const add = (path: string) => {
    if (files.some((f) => f.path === path)) return;
    const next = [...files, { path, rxPosition: defaultRxPosition.trim() || null }];
    onChange(next);
    check(next);
  };

  const addFile = async () => {
    try {
      const picked = await open({
        multiple: false,
        title: "Choose a WSJT-X ALL.TXT log",
        filters: [{ name: "WSJT-X log", extensions: ["txt", "TXT"] }],
      });
      if (typeof picked === "string") add(picked);
    } catch (e) {
      setError(String(e));
    }
  };

  const findLogs = async () => {
    try {
      setFound(await invoke<FoundLog[]>("find_log_files"));
    } catch (e) {
      setError(String(e));
    }
  };

  const notAdded = found?.filter((f) => !files.some((o) => o.path === f.path)) ?? [];

  return (
    <>
      <h3 style={{ marginTop: 22 }}>WSJT-X logs</h3>
      <p className="note">
        Each WSJT-X installation keeps its own ALL.TXT log of everything it decoded. Add the ones
        you have, old and new. They are read when the app starts and when you press Check now, new
        lines only. The log does not say where the receiver was, so give each one a position to
        get distances and bearings.
      </p>
      <div className="controls">
        <button type="button" onClick={addFile} disabled={busy}>
          Add log file…
        </button>
        <button type="button" onClick={findLogs} disabled={busy}>
          Find logs
        </button>
        <button type="button" onClick={() => check(files)} disabled={busy || files.length === 0}>
          {busy ? "Checking…" : "Check now"}
        </button>
      </div>
      {error && <p className="error">{error}</p>}
      {found &&
        (notAdded.length === 0 ? (
          <p className="hint">No other logs found in the usual program folders.</p>
        ) : (
          <ul className="found-logs">
            {notAdded.map((f) => (
              <li key={f.path}>
                <strong>{f.program}</strong> <span className="mono">{f.path}</span> <span className="hint">{size(f.sizeBytes)}
                {f.modifiedUtc !== null && `, last written ${stampBoth(f.modifiedUtc)}`}</span>{" "}
                <button type="button" onClick={() => add(f.path)} disabled={busy}>
                  Add
                </button>
              </li>
            ))}
          </ul>
        ))}
      {files.length === 0 ? (
        <p className="hint">No logs added yet.</p>
      ) : (
        <div className="scroll-x">
          <table className="results compact logs">
            <thead>
              <tr>
                <th>Log</th>
                <th>Receiver position</th>
                <th>Last check</th>
                <th></th>
              </tr>
            </thead>
            <tbody>
              {files.map((f) => {
                const c = checks.find((check) => check.path === f.path);
                return (
                  <tr key={f.path}>
                    <td className="left mono">{f.path}</td>
                    <td>
                      <input
                        className="short"
                        placeholder="EM73"
                        value={f.rxPosition ?? ""}
                        onChange={(e) =>
                          onChange(
                            files.map((o) =>
                              o.path === f.path ? { ...o, rxPosition: e.target.value || null } : o,
                            ),
                          )
                        }
                      />
                    </td>
                    <td className={`left${c && !c.ok ? " error" : ""}`}>
                      {c ? `${c.detail}, ${stampBoth(c.checkedUtc)}` : "not checked yet"}
                    </td>
                    <td>
                      <button type="button" onClick={() => onChange(files.filter((o) => o.path !== f.path))}>
                        Remove
                      </button>
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        </div>
      )}
    </>
  );
}
