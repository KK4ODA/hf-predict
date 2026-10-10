import { useState } from "react";
import { StationEditor } from "./StationEditor";
import { UpdateCheck } from "./UpdateCheck";
import { Options, SavedLocation, StationProfile, UserData } from "./types";
import atkinsonLicence from "@fontsource/atkinson-hyperlegible-next/LICENSE?raw";
import barlowLicence from "@fontsource/barlow-semi-condensed/LICENSE?raw";

export type Theme = "dark" | "light";

type Props = {
  options: Options;
  userData: UserData;
  onUserData: (data: UserData) => void;
  txStation: StationProfile;
  rxStation: StationProfile;
  onTxStation: (station: StationProfile) => void;
  onRxStation: (station: StationProfile) => void;
  onUseLocation: (position: string, end: "from" | "to") => void;
  theme: Theme;
  onTheme: (theme: Theme) => void;
  version: string;
};

const SHORTCUTS: [string, string][] = [
  ["Ctrl+Enter", "Predict"],
  ["Alt+1 to Alt+0", "Switch views, in the order of the list on the left"],
  ["[ and ]", "Previous and next hour"],
  ["n", "Back to the current hour"],
];

/** Stations, saved places, appearance and updates. */
export function SetupPanel(props: Props) {
  const { options, userData, onUserData, txStation, rxStation, onTxStation, onRxStation, onUseLocation, theme, onTheme, version } = props;
  const [newLocation, setNewLocation] = useState<SavedLocation>({ name: "", position: "" });

  const saveStation = (station: StationProfile) => {
    const others = userData.stations.filter((s) => s.name !== station.name);
    onUserData({ ...userData, stations: [...others, station] });
  };
  const addLocation = () => {
    const location = { name: newLocation.name.trim(), position: newLocation.position.trim() };
    const others = userData.locations.filter((l) => l.name !== location.name);
    onUserData({ ...userData, locations: [...others, location] });
    setNewLocation({ name: "", position: "" });
  };

  return (
    <section>
      <div className="view-head">
        <h2>Stations and settings</h2>
      </div>

      <div className="grid-2" style={{ maxWidth: 900 }}>
        <StationEditor
          title="My station, transmitting"
          station={txStation}
          options={options}
          saved={userData.stations}
          onChange={onTxStation}
          onSave={saveStation}
        />
        <StationEditor
          title="Other station, receiving"
          station={rxStation}
          options={options}
          saved={userData.stations}
          onChange={onRxStation}
          onSave={saveStation}
        />
      </div>

      <h3>Saved places</h3>
      <p className="note">Saved places appear as suggestions in From and To.</p>
      {userData.locations.length > 0 && (
        <table className="data" style={{ minWidth: 520 }}>
          <thead>
            <tr>
              <th className="left">Name</th>
              <th className="left">Position</th>
              <th />
            </tr>
          </thead>
          <tbody>
            {userData.locations.map((l) => (
              <tr key={l.name}>
                <th>{l.name}</th>
                <td className="left num">{l.position}</td>
                <td>
                  <button type="button" className="quiet" onClick={() => onUseLocation(l.position, "from")}>
                    Use as From
                  </button>
                  <button type="button" className="quiet" onClick={() => onUseLocation(l.position, "to")}>
                    Use as To
                  </button>
                  <button
                    type="button"
                    className="quiet"
                    onClick={() => onUserData({ ...userData, locations: userData.locations.filter((o) => o.name !== l.name) })}
                  >
                    Remove
                  </button>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
      <div className="row" style={{ maxWidth: 520, marginTop: 8 }}>
        <input placeholder="Name" value={newLocation.name} onChange={(e) => setNewLocation({ ...newLocation, name: e.target.value })} />
        <input
          placeholder="Locator or latitude, longitude"
          value={newLocation.position}
          onChange={(e) => setNewLocation({ ...newLocation, position: e.target.value })}
        />
        <button type="button" disabled={!newLocation.name.trim() || !newLocation.position.trim()} onClick={addLocation}>
          Save place
        </button>
      </div>

      <h3>Appearance</h3>
      <div className="segmented" role="group" aria-label="Theme">
        <button type="button" aria-pressed={theme === "dark"} onClick={() => onTheme("dark")}>
          Dark
        </button>
        <button type="button" aria-pressed={theme === "light"} onClick={() => onTheme("light")}>
          Daylight
        </button>
      </div>
      <p className="note">Daylight is easier to read outdoors and in bright rooms.</p>

      <h3>Keyboard</h3>
      <dl className="shortcuts">
        {SHORTCUTS.map(([keys, what]) => (
          <div key={keys} style={{ display: "contents" }}>
            <dt>
              <kbd>{keys}</kbd>
            </dt>
            <dd>{what}</dd>
          </div>
        ))}
      </dl>

      <h3>Updates</h3>
      <p className="note">This is version {version}. Updates come from the project's GitHub releases and are signed.</p>
      <UpdateCheck />

      <h3>About</h3>
      <p className="note">
        HF Predict is free software under the Apache License 2.0. Predictions come from VOACAP, the
        NTIA/ITS propagation model, run locally.
      </p>
      <details>
        <summary>Third-party licences</summary>
        <p className="note">
          The typefaces are Atkinson Hyperlegible Next and Barlow Semi Condensed, both under the SIL
          Open Font License 1.1.
        </p>
        <pre>{atkinsonLicence}</pre>
        <pre>{barlowLicence}</pre>
      </details>
    </section>
  );
}
