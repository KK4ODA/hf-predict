import { useState } from "react";
import { Options, StationProfile } from "./types";

type Props = {
  title: string;
  station: StationProfile;
  options: Options;
  saved: StationProfile[];
  onChange: (station: StationProfile) => void;
  onSave: (station: StationProfile) => void;
};

/** Picks a preset or saved station and lets its values be adjusted. */
export function StationEditor({ title, station, options, saved, onChange, onSave }: Props) {
  const [saveName, setSaveName] = useState("");
  const profiles = [...options.presets, ...saved];
  const set = (change: Partial<StationProfile>) => onChange({ ...station, ...change });

  return (
    <fieldset className="panel">
      <legend>{title}</legend>
      <label>
        Start from
        <select
          value={profiles.some((p) => p.name === station.name) ? station.name : ""}
          onChange={(e) => {
            const picked = profiles.find((p) => p.name === e.target.value);
            if (picked) onChange(picked);
          }}
        >
          <option value="" disabled>
            {station.name} (edited)
          </option>
          <optgroup label="Presets">
            {options.presets.map((p) => (
              <option key={p.name}>{p.name}</option>
            ))}
          </optgroup>
          {saved.length > 0 && (
            <optgroup label="Saved">
              {saved.map((p) => (
                <option key={p.name}>{p.name}</option>
              ))}
            </optgroup>
          )}
        </select>
      </label>
      <div className="row">
        <label>
          Power, W
          <input
            type="number"
            min={0.1}
            step="any"
            value={station.powerWatts}
            onChange={(e) => set({ powerWatts: Number(e.target.value) })}
          />
        </label>
        <label>
          Lowest take-off angle, °
          <input
            type="number"
            min={0.1}
            max={40}
            step={0.1}
            value={station.minAngleDeg}
            onChange={(e) => set({ minAngleDeg: Number(e.target.value) })}
          />
        </label>
      </div>
      <label>
        Antenna
        <select value={station.antenna} onChange={(e) => set({ antenna: e.target.value })}>
          {options.antennas.map((a) => (
            <option key={a.value} value={a.value}>
              {a.label}
            </option>
          ))}
        </select>
      </label>
      <label>
        Local noise
        <select value={station.noiseDb} onChange={(e) => set({ noiseDb: Number(e.target.value) })}>
          {options.noiseLevels.map((n) => (
            <option key={n.value} value={n.value}>
              {n.label}
            </option>
          ))}
        </select>
      </label>
      <div className="row">
        <input placeholder="Name to save as" value={saveName} onChange={(e) => setSaveName(e.target.value)} />
        <button
          type="button"
          disabled={!saveName.trim()}
          onClick={() => {
            const named = { ...station, name: saveName.trim() };
            onSave(named);
            onChange(named);
            setSaveName("");
          }}
        >
          Save station
        </button>
      </div>
    </fieldset>
  );
}
