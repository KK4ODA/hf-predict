import { useState } from "react";
import { StationEditor } from "./StationEditor";
import { UpdateCheck } from "./UpdateCheck";
import { Options, SavedLocation, StationProfile, UserData } from "./types";
import atkinsonLicence from "@fontsource/atkinson-hyperlegible-next/LICENSE?raw";
import barlowLicence from "@fontsource/barlow-semi-condensed/LICENSE?raw";
import voacaplLicence from "../engines/voacapl/LICENSE-voacapl.txt?raw";
import ntiaNotice from "../engines/voacapl/NOTICE-NTIA.txt?raw";

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

      <h3>About and credits</h3>
      <p className="note">HF Predict is free software under the Apache License 2.0.</p>
      <dl className="credits">
        <dt>VOACAP</dt>
        <dd>
          The propagation model behind every prediction. Developed for the Voice of America from IONCAP,
          the HF model of the Institute for Telecommunication Sciences (NTIA/ITS), with work by the Naval
          Research Laboratory. Theory by John Lloyd, George Haydon, Donald Lucas and Larry Teters;
          development steered at the Voice of America by George Lane; major improvements by Franklin
          Rhoads of NRL; many later features designed, and the code maintained, by Greg Hand of NTIA/ITS.
        </dd>
        <dt>voacapl</dt>
        <dd>
          The gfortran port of VOACAP by Jim Watson, HZ1JW / M0DNS, which HF Predict builds for Windows,
          macOS and Linux and runs as its engine. github.com/jawatson/voacapl
        </dd>
        <dt>VOACAP Online</dt>
        <dd>
          By Jari Perkiömäki, OH6BG, launched with Jim Watson, HZ1JW, and Juho Juopperi, OH8GLV. It has long
          made VOACAP usable by radio amateurs, and its guides informed choices here, such as using current
          published sunspot numbers. HF Predict does not use the service. voacap.com
        </dd>
        <dt>Also</dt>
        <dd>
          The WSJT-X development group, whose published UDP messages HF Predict listens to; Hamlib, for
          rigctld; NOAA's Space Weather Prediction Center, for solar data; Natural Earth map data; the
          Atkinson Hyperlegible Next and Barlow typefaces.
        </dd>
      </dl>
      <p className="note">
        HF Predict is independent: it is not affiliated with or endorsed by NTIA/ITS, the Voice of America,
        the U.S. Government, VOACAP Online or the WSJT-X development group.
      </p>
      <details>
        <summary>Notices and licences</summary>
        <h4>VOACAP, from NTIA/ITS</h4>
        <pre>{ntiaNotice}</pre>
        <h4>voacapl</h4>
        <pre>{voacaplLicence}</pre>
        <h4>Typefaces, SIL Open Font License 1.1</h4>
        <pre>{atkinsonLicence}</pre>
        <pre>{barlowLicence}</pre>
      </details>
    </section>
  );
}
