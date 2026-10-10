import { ReactNode } from "react";
import { localClock, zoneAt } from "./localtime";
import { age, Dot, Health } from "./ui";
import { ClockCheck, Conditions, ListenerStatus, PathDetail, RadioStatus, ScanStatus } from "./types";

export type Live = {
  listener: ListenerStatus | null;
  radio: RadioStatus | null;
  scan: ScanStatus | null;
};

type Props = {
  version: string;
  detail: PathDetail | null;
  longPath: boolean;
  conditions: Conditions | null;
  live: Live;
  nowS: number;
  onNavigate: (view: "conditions" | "heard" | "radio" | "plan") => void;
  onEditPath: () => void;
  /** The best band now, when a path has been predicted. */
  children?: ReactNode;
};

function receiver(listener: ListenerStatus | null): { state: Health; value: string; sub: string } {
  if (!listener || listener.state === "off") return { state: "off", value: "Off", sub: "" };
  if (listener.state === "failed") return { state: "alert", value: "Error", sub: "" };
  const decoder = listener.tracker?.decoders[0];
  if (!decoder || decoder.secondsSinceHeard > 30) return { state: "warn", value: "Waiting", sub: "no WSJT-X" };
  if (decoder.transmitting) return { state: "alert", value: decoder.band ?? "?", sub: "transmitting" };
  return { state: "ok", value: decoder.band ?? "?", sub: decoder.mode ?? "" };
}

function radio(status: RadioStatus | null, scan: ScanStatus | null): { state: Health; value: string; sub: string } {
  if (scan && (scan.state === "running" || scan.state === "paused")) {
    return {
      state: scan.state === "paused" ? "warn" : "busy",
      value: scan.current?.band ?? "…",
      sub: scan.state === "paused" ? "scan paused" : "scanning",
    };
  }
  if (!status || status.state === "off") return { state: "off", value: "Off", sub: "" };
  if (status.state === "failed") return { state: "alert", value: "Error", sub: "" };
  if (status.state === "connecting" || !status.radio) return { state: "warn", value: "Connecting", sub: "" };
  return {
    state: status.radio.ptt ? "alert" : "ok",
    value: (status.radio.freqHz / 1e6).toFixed(3),
    sub: status.radio.ptt ? "transmitting" : status.radio.mode,
  };
}

/** Readouts that matter on every screen: path, time, sun, receiver, radio. */
export function StatusBar(props: Props) {
  const { version, detail, longPath, conditions, live, nowS, onNavigate, onEditPath, children } = props;
  const zone = zoneAt(new Date(nowS * 1000));
  const utc = new Date(nowS * 1000).toISOString().slice(11, 19);
  const wwv = conditions?.products.find((p) => p.kind === "wwv");
  const sun = wwv?.stored?.product.kind === "wwv" ? wwv.stored.product : null;
  const rx = receiver(live.listener);
  const rig = radio(live.radio, live.scan);
  const clock: ClockCheck | null = live.listener?.state === "listening" ? (live.listener.tracker?.clock ?? null) : null;
  const clockOff = clock && (clock.level === "warn" || clock.level === "alarm");

  return (
    <header className="statusbar">
      <div className="sb-brand">
        <strong>HF Predict</strong>
        <span className="version sb-opt2">{version}</span>
      </div>

      <button type="button" className="sb-cell grow" onClick={onEditPath} title="Change the path">
        {detail ? (
          <>
            <span className="sb-value">
              {detail.prediction.txLocator} → {detail.prediction.rxLocator}
            </span>
            <span className="sb-sub sb-opt1">{detail.prediction.distanceKm.toFixed(0)} km</span>
            <span className="sb-sub sb-opt1">{detail.prediction.txBearingDeg.toFixed(0)}°</span>
            <span className="sb-label sb-opt2">{longPath ? "long path" : "short path"}</span>
          </>
        ) : (
          <span className="sb-label">No path predicted yet</span>
        )}
      </button>

      <div className="sb-cell static" title="Current time">
        <span className="sb-value">{utc}</span>
        <span className="sb-label">UTC</span>
        <span className="sb-sub sb-opt3">{localClock(nowS, false)}</span>
        <span className="sb-label sb-opt3">{zone.name}</span>
      </div>

      <button
        type="button"
        className={`sb-cell${!sun || wwv?.stale ? " sb-stale" : ""}`}
        onClick={() => onNavigate("conditions")}
        title={sun ? `Solar flux, A and K index from the WWV bulletin` : "No solar data yet"}
      >
        {sun ? (
          <>
            <span className="sb-label">SFI</span>
            <span className="sb-value">{sun.solarFlux ?? "?"}</span>
            <span className="sb-label">A</span>
            <span className="sb-value">{sun.aIndex ?? "?"}</span>
            <span className="sb-label">K</span>
            <span className="sb-value">{sun.kIndex ?? "?"}</span>
            {wwv?.ageSeconds != null && (
              <span className={wwv.stale ? "sb-label caution" : "sb-label sb-opt1"}>{age(wwv.ageSeconds)} old</span>
            )}
          </>
        ) : (
          <span className="sb-label">No solar data</span>
        )}
      </button>

      <button type="button" className="sb-cell" onClick={() => onNavigate("heard")} title="WSJT-X decodes received">
        <Dot state={rx.state} />
        <span className="sb-label">Receiver</span>
        <span className="sb-value small">{rx.value}</span>
        {rx.sub && <span className="sb-sub sb-opt1">{rx.sub}</span>}
      </button>

      <button type="button" className="sb-cell" onClick={() => onNavigate(rig.sub.startsWith("scan") ? "plan" : "radio")} title="Radio through rigctld">
        <Dot state={rig.state} />
        <span className="sb-label">Radio</span>
        <span className="sb-value small">{rig.value}</span>
        {rig.sub && <span className="sb-sub sb-opt1">{rig.sub}</span>}
      </button>

      {clockOff && clock && (
        <button type="button" className="sb-cell" onClick={() => onNavigate("heard")} title="This computer's clock looks off">
          <Dot state="alert" />
          <span className="sb-label">Clock</span>
          <span className="sb-value small">
            {clock.medianDtS !== null ? `${clock.medianDtS > 0 ? "+" : ""}${clock.medianDtS.toFixed(1)} s` : "off"}
          </span>
        </button>
      )}

      {children}
    </header>
  );
}
