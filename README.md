# hf-predict

Offline-first HF propagation prediction and measurement for amateur radio, for Windows, Linux and macOS.

- **Predict** — model what HF propagation should be doing, using an established propagation engine running locally.
- **Measure** — passively observe on-air FT8 signals to see what the station is actually hearing.
- **Compare** — show where prediction and observation agree or disagree, and turn that into a band recommendation.

The application is designed to stay useful with little or no Internet connectivity: portable operation, Field Day, EmComm and disaster response.

## Status

Early development. Installers are on the [Releases](https://github.com/KK4ODA/hf-predict/releases) page; builds are not code-signed yet, so Windows and macOS warn on install.

Working now:

- Offline point-to-point prediction for the 80 m to 10 m amateur bands, hour by hour, using the real VOACAP engine bundled with the app.
- Best bands for any hour, short and long path, a chart of the usable frequency range through the day, and a transmit-power comparison.
- A world map for picking either end of the path, with day and night and a coverage overlay showing where a band reaches.
- Station presets (power, antenna, local noise), saved stations and saved locations.
- A bundled NOAA smoothed sunspot table, so no network is needed. A newer one is downloaded when there is a network.
- Solar and geophysical conditions (flux, A and K indices, storm state, three-day and 27-day forecasts) with source and age. They can be fetched from NOAA, or requested over Winlink and imported from files or pasted text.
- A listening plan: which bands to listen on, in what order and for how long, from the prediction, what was heard in the last hour and how long each band has gone unsampled; bands to tick in WSJT-X's band hopping, and a schedule to follow by hand. Nothing transmits or moves the radio.
- Reads the radio's frequency, mode, PTT, split and VFO through a `rigctld` shared with WSJT-X, which the app can start and stop itself, and checks that both see the same dial. The app has no transmit function and does not change the radio in this version.
- Passive reception of WSJT-X decodes over the network (shared with GridTracker and JTAlert through a multicast group), stored locally with per-band activity and a clock check; the ALL.TXT logs of any number of WSJT-X installations are read in place, new lines only.
- A comparison of prediction with what has been heard toward the destination, a per-band recommendation, and a simplified Field screen.
- In-app update check.

The [engineering assessment](docs/engineering-assessment.md) covers engine selection, licensing, WSJT-X integration, CAT architecture and the roadmap.

## Building

Needs Rust, Node.js, gfortran, make, autoconf and automake. On Windows, run the first command in an MSYS2 UCRT64 shell.

```sh
sh engines/voacapl/build.sh      # builds the VOACAP engine into .work/engine
npm install
npm run tauri dev                # or: npm run tauri build
```

Tests: `sh tests/engine/run-reference.sh` and `cargo test --manifest-path src-tauri/Cargo.toml`.

## License

Apache-2.0. See [LICENSE](LICENSE). Third-party components and their terms are listed in the engineering assessment.
