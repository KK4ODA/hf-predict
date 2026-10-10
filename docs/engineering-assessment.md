# hf-predict — Engineering Assessment

Date: 2026-10-04. Status: pre-implementation; no application code exists yet.

Facts in this document were checked against primary sources (project repositories, source code, official documentation) on the date above. Anything not confirmed is marked **(unverified)**. Design proposals are labelled as proposals. Sources are listed in section 20.

## Summary of recommendations

| Topic | Recommendation |
|---|---|
| Propagation engine | Run real VOACAP locally as a subprocess (`voacapl`, the maintained port of the NTIA Fortran code), behind a `PropagationEngine` interface. |
| Windows engine | Resolved in Phase 0: `voacapl` builds natively on Windows and its output is identical to the Linux build (section 1). |
| Second engine | ITURHFProp (ITU-R P.533) later and optional. Its licence grant is narrow, its data is about 138 MB, and a macOS build is unverified. |
| Solar input | VOACAP consumes only the monthly smoothed sunspot number. Bundle a NOAA SWPC observed and predicted table. Everything else (Kp, A, flux, X-ray) is operator awareness, not model input. |
| FT8 observations | Listen to WSJT-X's UDP stream over multicast. Observe-only first. No decoder of our own until there is a strong reason. |
| Radio control | Later phase. Shared `rigctld` with WSJT-X as a co-client. WSJT-X has no UDP message for changing frequency. |
| Before CAT | Use the FT8 band hopping built into WSJT-X 3.0 with a hop list our app recommends. No CAT risk. |
| Winlink | Request four small text products from the PROPAGATION catalog (about 8 kB as delivered). Import by folder watch, file open or paste. |
| Application stack | Tauri 2 (Rust core, TypeScript UI), SQLite, bundled offline map data. |
| Distribution | Public GitHub repository, GitHub Actions builds for three operating systems, signed in-app updater against GitHub Releases, and an offline installer path. |
| App licence | Apache-2.0 (decided, section 19). |

---

## 1. VOACAP local-execution feasibility

**Verdict: feasible on all three systems. Confirmed by the Phase 0 build spike.**

### Phase 0 results (2026-10-04)

- **One script builds the engine on Linux x64, macOS arm64 and Windows x64** (`engines/voacapl/build.sh`; MinGW-w64 gfortran under MSYS2 on Windows). The Fortran runtime is linked in, so no compiler is needed on the user's machine.
- **Agreement with Windows VOACAP 16.1207.** On the reference case shipped with `voacapl`, 5,287 of 5,431 numeric fields are identical, 143 differ by one unit in the last printed digit, and one (a virtual height, 398 against 400 km) differs by 0.5%. Nothing differs by more.
- **Agreement between systems.** Windows and Linux outputs are identical on all four test decks. macOS arm64 differs from Linux in at most 7 last-digit fields per deck.
- **Paths.** Folders with spaces work. A data or run folder path beyond about 128 characters fails with a clear error. The data tree can be named by a relative path, so the app runs the engine from the engine folder and only the run folder's absolute path is subject to the limit.
- **Licence.** The GPL-3 option parser is linked only into the two `dst` utilities, which the build leaves out. The engine binary contains public-domain and CC0 code only.
- **Not done.** Only one test deck has a Windows VOACAP reference output. More need a Windows VOACAP installation to generate them. The alternative engines were not evaluated, since no fallback is needed.

The feasibility notes below are as written before the spike.

### What exists

- **Original NTIA/ITS VOACAP** is distributed for Windows by Greg Hand as HFWin32 (`itshfbc`), containing VOACAP, ICEPAC and REC533. The advertised latest build is 16.1207 (December 2016); the versions directory also holds April 2018 builds. Fortran source for VOACAP and the coefficient files are downloadable from the same site. The NTIA/ITS copy is older (2005–2008).
- **`voacapl`** (James Watson) is the same Fortran code adapted to compile with gfortran. Latest release is v0.7.7 (March 2026). It has been aligned with Windows build 16.1207 since v0.7.2. Its documentation states that only compile-compatibility changes were made and the algorithms are unchanged. Debian ships it in `main` for amd64, arm64, armhf and riscv64.
- No official GitHub repository for the original source was found (unverified absence).

### How it runs

- Invocation: `voacapl [-s] [--run-dir=…] <itshfbc-dir> [input.dat] [output.out]`. There are also `area calc` and `batch` forms.
- Input is a text "deck" of cards: `TIME`, `MONTH`, `SUNSPOT`, `CIRCUIT` (with a short/long path flag), `SYSTEM` (power, noise, minimum angle, required reliability, required SNR), `ANTENNA`, `FREQUENCY` (up to 11 per card), `METHOD`, `EXECUTE`.
- Output is a fixed-format text file with, per hour and frequency: `MODE`, `TANGLE` (takeoff angle), `DELAY`, `V HITE`, `MUFday`, `LOSS`, `DBU`, `S DBW`, `N DBW`, `SNR`, `RPWRG`, `REL`, `MPROB`, `SNR LW/UP`, `TGAIN`, `RGAIN`, `SNRxx`.
- Data tree: about 7.9 MB installed (1.4 MB compressed). Binary: about 1 MB.
- Speed: a published benchmark gives about 370 circuits per second on 32 cores for 24-hour single-frequency decks. That implies roughly 90 ms per circuit per core (unverified inference). A ten-band, 24-hour point-to-point prediction is one run.
- Method 30 is the recommended method; it smooths between the short-path and long-path models at 7,000–10,000 km. Methods 21 and 22 force long and short.

### Constraints that shape the integration

- **Path limits.** Behaviour is described as unpredictable if the `itshfbc` path contains spaces or exceeds 52 characters. A third-party report puts an internal buffer at 128 characters. Default data directories on Windows (`C:\Users\First Last\…`) and macOS (`~/Library/Application Support/…`) can both contain spaces. The app must place the engine data in a short, space-free location and test for this at start-up.
- **Open bugs (September 2026).** `--silent` is not recognised (use `-s`). `batch` mode silently processes nothing on case-sensitive file systems. We avoid `batch` and loop in our own code.
- **Unvalidated outputs.** The VOACAP documentation says only Methods 13, 14, 15, 20, 21, 22 and 25 were benchmarked; the HPF/FOT/LUF methods were never tested. Proposal: derive those quantities ourselves from a frequency sweep (section 4) instead of presenting Method 9 output as authoritative.
- **Frequency range.** VOACAP is a 2–30 MHz model. Tested in Phase 1: the engine accepts 1.84 MHz and returns numbers, but parts of its code clamp frequencies to 2–30 MHz, so those numbers are outside the model's range. The app treats 160 m and 6 m as observation-only bands.
- **Sporadic E** is off by default in VOACAP because that part of the model is not fully tested. Unexpected high-band openings are exactly what the FT8 measurement side is for.

### Windows options, in order of preference

1. **Build `voacapl` with MinGW-w64 gfortran (MSYS2).** One codebase and a clean licence on all three platforms. No evidence anyone has done it; this is the project's first spike.
2. **`hfcast-engine`** (Rust, Apache-2.0, created August 2026). Claims a complete port of the Fortran with zero differing cells across 463,104 comparisons against `voacapl`. It is two months old with no independent validation. If it passes our own reference cases it becomes attractive on every platform, because it runs in-process with no Fortran toolchain and no path limits.
3. **DVOACAP `dvoa.dll`** (VE3NEA, MIT). A re-implementation, not a port; matches VOACAP closely but not bit-for-bit, has no sporadic-E layer and only isotropic antennas natively. Winlink Express has shipped it since 2018, which is a working precedent.
4. **Drive a user-installed HFWin32** (`voacapw.exe SILENT …`). Proven headless, but it is 32-bit, needs a space-free install path, and we would not redistribute it (see section 3).

### Integration design (proposal)

- One engine process per prediction, with its own run directory, a timeout, and captured output.
- Decks are generated from a typed request; outputs are parsed into typed results; both files are kept on failure for diagnosis.
- Results are cached by a hash of all inputs plus engine name and version.

---

## 2. Propagation-engine comparison

| Engine | What it is | Licence | Platforms | Fit |
|---|---|---|---|---|
| `voacapl` 0.7.7 | Original VOACAP Fortran, gfortran build | Engine code public domain / CC0; three helper files GPL-3 | Linux, macOS; Windows unproven | **Primary** |
| HFWin32 `voacapw.exe` | Original Windows build | NTIA: free to use; parts of the package are not redistributable | Windows 32-bit | Reference for golden test cases; last-resort Windows fallback |
| `hfcast-engine` | Rust port of VOACAP | Apache-2.0 | Any | Candidate; validate ourselves |
| DVOACAP `dvoa.dll` | Delphi re-implementation, JSON interface | MIT | Windows, Linux | Windows fallback |
| `dvoacap-python` 1.0.2 | Python port of DVOACAP | MIT | Any | Not suitable: loose tolerances, baselines are self-regression |
| ITURHFProp 14.3 | ITU-R P.533-14 in C | ITU grant, no licence file | Windows, Linux; macOS unverified | Optional second engine |
| ICEPAC | VOACAP sibling with polar model | As HFWin32 | Windows | No: its own author advises caution |
| PHaRLAP / PyLap | Ray tracing | By request from DST Australia | Linux | No: not redistributable, not operational |
| IRI-2020, NeQuick | Ionosphere climatology | Open with conditions | — | Not circuit predictors |
| W6ELProp, HamCAP, ASAPS | Closed or commercial | — | Windows | Not reusable |

**Why VOACAP over P.533 as primary**

- It is the model hams already calibrate their expectations against (VOACAP Online, HamCAP, Winlink Express).
- Its outputs include reliability and SNR distributions, not only medians.
- It is small (8 MB against about 138 MB).
- Its licence position is clearer.
- Published comparisons are mixed rather than decisive: some report P.533-type methods better in winter and for NVIS, VOACAP better in summer (unverified, abstracts only).

**Why keep P.533 in view.** VOACAP Online itself runs both engines side by side. A second independent model is useful when the two disagree. The `PropagationEngine` interface makes it an add-on, not a redesign.

**Not cloned.** VOACAP Online prohibits automated access, so it is used only for manual spot comparisons of a few circuits.

---

## 3. Licensing and attribution

| Component | How we use it | Licence | Obligation |
|---|---|---|---|
| VOACAP engine source (NTIA/ITS) | Bundle compiled binary and data | Not subject to US copyright; ITS grants use, copy, modify, redistribute | Ship both NTIA notices verbatim; no warranty; do not imply endorsement |
| `voacapl` changes | Bundle | CC0 | Credit James Watson (courtesy) |
| `dst2csv`, `dst2ascii`, `f90getopt.f90` in `voacapl` | **Leave out** | GPL-3 | Confirmed in Phase 0: `f90getopt.f90` is linked only into the two `dst` utilities, not the engine. The build script removes them. |
| HFWin32 installer, `SALFLIBC.DLL`, GUI | **Do not redistribute** | Package help says most programs cannot be distributed | Detect a user install only |
| DVOACAP | Optional bundle | MIT | Include licence text |
| `hfcast-engine` | Optional link | Apache-2.0 | Include licence and notices |
| ITURHFProp | Optional, later | README grant scoped to implementing the Recommendation; no redistribution clause | Ask ITU-R SG3 before bundling |
| WSJT-X UDP stream | Consume over a socket | WSJT-X is GPLv3 | None from consuming output. BSD (GridTracker2), MIT, Apache and proprietary (JTAlert) clients already exist. We write the parser from the published message layout and acknowledge the WSJT-X Development Group. |
| `rigctld` (Hamlib utilities) | Separate process | GPL-2.0-or-later (library is LGPL-2.1) | Prefer an installed `rigctld` or the `rigctld-wsjtx` that ships with WSJT-X. If we bundle it: include licence texts and offer source. |
| NOAA SWPC data | Bundle and fetch | Public domain | Do not present altered content as official; no endorsement |
| SILSO sunspot data | **Do not bundle** | CC BY-NC 4.0 | Use SWPC's tables instead |
| GIRO ionosonde data, KC2G | **Do not bundle** | Non-commercial, account-holders only | Online display at most, later |
| PSK Reporter, RBN, wspr.live | Optional online queries | Usage policies | Rate limits in section 13 |
| Map data (Natural Earth) | Bundle | Public domain | None |
| Callsign prefix table | Bundle | **(unverified)** | Check terms before bundling |

None of this is legal advice. The two ambiguous items are the non-US copyright status of the NTIA code and the ITU grant.

---

## 4. Offline data requirements

### Shipped with the app (all usable with no network)

| Data | Size | Purpose |
|---|---|---|
| `itshfbc` tree: CCIR and URSI coefficients, about 70 antenna files | about 8 MB | Engine |
| Smoothed sunspot table: SWPC observed history and prediction | under 100 kB | The only solar input the engine needs |
| World map vectors, Maidenhead grid, terminator maths | a few MB | Maps |
| Callsign prefix to country table | under 1 MB | Locating stations that send no grid |
| FT8 dial frequencies per band | trivial | Band identification and scanning |

### Two prediction classes, always labelled

- **Climatological.** Date, time, locations, station profile, bundled smoothed sunspot number. Works indefinitely offline. As the bundled prediction table ages, the app says so.
- **Adjusted.** Same model, with the sunspot input refreshed from newer data. Any adjustment from short-term indices is marked experimental (below).

### What each parameter actually does

| Parameter | VOACAP | P.533 | Role in this app |
|---|---|---|---|
| Monthly smoothed sunspot number (R12) | **Input** | **Input** | Drives predictions |
| Month, UTC hour, path geometry | **Input** | **Input** | Drives predictions |
| Power, antennas, noise, required SNR | **Input** | **Input** | Station profile |
| Daily sunspot number | Not an input | Not an input | Awareness; VOACAP's authors say never to feed it in |
| F10.7 daily flux | Not an input | Not an input | Awareness; a multi-week mean is usable as a drift check |
| Kp, K, A indices | Not inputs (ICEPAC alone takes a K-derived Q index) | Not inputs | Storm warning banner |
| X-ray flux, flares, proton events | Not inputs | Not inputs | Blackout warning on sunlit paths |
| D-region absorption (D-RAP) | Not an input | Not an input | Awareness, online only |
| foF2 from ionosondes | Not an input | Not an input | Not used (data terms) |

### Open scientific question: the sunspot scale

VOACAP's maps were fitted to the pre-2015 sunspot scale. Since 2015 the published numbers are roughly 40–45% higher for the modern era. One published estimate puts the effect at about one band up or down. voacap.com still points users to current published predictions. Proposal: default to the published (new-scale) values for compatibility with VOACAP Online, expose the choice as an advanced setting, and let our own prediction-versus-observation history inform it later.

### Near-real-time adjustment (proposal, experimental)

- Precedents: VOACAP Online offers an experimental "dynamic" sunspot number (3-day mean of daily values). Winlink Express blends current flux with its predicted table.
- Our proposal: use a multi-week mean of F10.7 to compute an equivalent sunspot number through a standard regression **(relation to be chosen and verified)**. Online, NOAA supplies observed means. Over Winlink, the 27-day outlook supplies 27 daily flux forecasts, and observed daily values accumulate locally with each import. If it differs from the bundled prediction by more than a threshold, show both and let the operator pick. This guards against a stale table more than it chases daily variation.
- Storms and flares never change the numbers. They add a plain-language warning to the affected bands and paths.
- NWRA's effective sunspot number feed, the usual source for this technique, was discontinued in May 2024.

### Derived quantities (proposal)

Run a frequency sweep and read the `MUFday` column (the fraction of days the path supports that frequency):

- MUF: frequency where `MUFday` is 50%.
- Optimum working frequency: where it is 90%.
- Highest probable frequency: where it is 10%.
- Lowest usable frequency: lowest frequency meeting the required reliability.

---

## 5. Winlink PROPAGATION integration

### How catalog requests work

- Send a message to `INQUIRY@winlink.org`, subject `REQUEST`, with one catalog ID per line. Each ID returns as a separate message from `SERVICE@winlink.org`. For items backed by a web address the subject is `INQUIRY - <source URL>`; other items use `INQUIRY: <ID>`.
- The reply body is MIME text, ISO-8859-1, quoted-printable: the NOAA product, then a line of `=====` and a Winlink footer. Confirmed from real replies received 2026-10-04.
- Winlink Express offers the same through Settings → Winlink Catalog Requests.

### What the PROPAGATION category contains

31 items, taken from the catalog file a Winlink Express installation keeps locally. No public web listing was found. The catalog does not give upstream URLs. The NOAA product behind each item is **confirmed** from real replies for the first five rows below and **inferred** from title and size for the rest.

| ID | Content | Size | Use to us |
|---|---|---|---|
| `PROP_WWV` | `wwv.txt`: solar flux, A index, K index, storm summary | 0.5 kB | Daily indices and storm state |
| `PROP_SGAS` | `sgas.txt`: daily flux, daily sunspot number, Ap, X-ray background, K indices, flare and proton events | 1 kB | Daily indices, flare events |
| `PROP_RSGA` | Listed as the joint USAF/NOAA report, but the reply is the same `sgas.txt` as `PROP_SGAS` | 1 kB | Not needed |
| `PROP3DNOAA` | `3-day-forecast.txt`: Kp by 3-hour block, storm and blackout probabilities | 1.8 kB | Forecast warnings |
| `PROP.27DO` | `27-day-outlook.txt`: flux, Ap, maximum Kp per day; issued weekly | 1.6 kB | Planning outlook; flux for the drift check |
| `PROP_3DAY`, `PROP_3DPROB`, `PROP_SGAS_27`, `PROPWKHI`, `PROP_ADVIS` | Further forecasts and advisories | 0.7–2.7 kB | Optional |
| `PROP_DRAP`, `PROP_SOLWIND`, ionograms, hourly area prediction charts | Images | 13–84 kB | Too large for routine HF use; not parsed |

**Key limitation: none of these carries the smoothed or predicted smoothed sunspot number.** Winlink gives us awareness data and the flux values for the drift check. It cannot refresh the model input directly.

### Proposed workflow

1. **Generate.** The app shows a ready-made request (`PROP_WWV`, `PROP_SGAS`, `PROP3DNOAA`, `PROP.27DO`; about 8 kB as delivered messages, less when compressed) to copy, or writes it as a message file for the user's Winlink client.
2. **Receive.** The user's own client and modem do the transfer. We never touch the modem.
3. **Detect.** Optional folder watch:
   - Winlink Express stores one MIME file per message under `<install>\<CALL>\Messages\`.
   - Pat stores one `.b2f` file per message under `mailbox/<CALL>/in/`.
   - The trigger is a new message from `SERVICE@winlink.org` whose subject starts `INQUIRY` and whose body carries a NOAA `:Product:` header.
4. **Fallbacks.** Open a file, or paste the text.
5. **Parse.** Parsers recognise each NOAA product by its own header lines, not by the Winlink ID. The same parsers serve the Internet path. They decode quoted-printable, cut the Winlink footer, and tolerate missing-value markers (`?`, `???`, `-1`, `*`) and decimal Kp.
6. **Store.** Each value is saved with source, transport (Winlink), issue time and import time.

### Airtime

Per the Winlink FAQ, 4 kB compressed takes about 15 minutes on Pactor 1, 4 minutes on Pactor 2, 30 seconds on Pactor 3, with ARDOP and VARA HF in between. The four-item request is practical on any of them.

### Extra compact datasets (proposal, needs testing)

- A GitHub Action in this repository could publish a daily "solar pack" of about 1 kB: the predicted smoothed sunspot numbers for the next 12 months plus recent indices.
- Online clients fetch it directly. Offline clients could fetch it over Winlink through Saildocs (`send <URL>` to `query@saildocs.com`), which the Winlink FAQ confirms is usable by Winlink users.
- This would close the gap noted above. Saildocs has no published size limit, does not follow redirects, and its handling of this file is untested.

### Still to confirm

- Whether any catalog item returns the joint USAF/NOAA report, which carries the 90-day mean flux.
- The upstream product behind the remaining IDs.

---

## 6. WSJT-X UDP protocol

Checked against the WSJT-X source at release 3.0.2 (June 2026) and 3.2.0-rc1.

### Wire format

- Binary datagrams: magic number `0xadbccbda`, schema number, message type, sender ID, then typed fields. Default destination `127.0.0.1:2237`.
- New message types and trailing fields are added without changing the schema number. A client must ignore unknown types and extra bytes.
- A passive listener receives schema 2 datagrams. The schema only rises if a server sends WSJT-X a higher one.

### Messages we need

| Type | Fields we use |
|---|---|
| Heartbeat (0) | Sent every 15 s and at start. Liveness and version. |
| Status (1) | Dial frequency (Hz), mode, DE callsign, DE grid, Tx enabled, transmitting, decoding, T/R period, configuration name |
| Decode (2) | New flag, time of day (no date), SNR, time offset DT, audio offset DF, mode symbol, message text, low-confidence flag, off-air flag |
| Clear (3), Close (6) | Session housekeeping; Close on graceful exit |
| WSPRDecode (10) | Callsign, grid, power, for WSPR sessions |

Types 16–18 exist in newer releases (annotation, inhibit); we ignore them.

Checked against live WSJT-X traffic on 2026-10-10: the message text in a Decode message is padded to the width of the decode window and may end in decoder notes, `?` for a low-confidence decode and `a1`–`a9` for the a-priori type used (for example `CQ W5RBD EL16                         a1`). A parser that treats the last token as the locator loses every CQ decoded with a-priori information; ours strips the notes first. Hashed callsigns keep their angle brackets (`<LU8VLW/V> W7ZR DM26`).

### What is and is not available

| Wanted | Available? |
|---|---|
| SNR, audio offset, UTC time, mode | Yes, in Decode |
| Dial frequency and band | Yes, from the most recent Status. Absolute frequency is dial plus audio offset. |
| Callsign, grid, CQ flag, message type | **Parse from the message text.** There are no separate fields. |
| Our own callsign and grid | Yes, in Status |
| Date | No. We add it from our UTC clock, with care around midnight. |

### Behaviours that matter

- **Decodes are withheld after a dial change.** WSJT-X 3.0 does not send decodes over UDP for 9 seconds after any dial-frequency change. Version 2.7 has no such guard, so a late decode can be attributed to the new band. We apply our own rule in all cases: a decode whose slot overlaps a dial change is flagged.
- **Decodes hidden by WSJT-X display filters are not sent.**
- **Inbound control is off by default** ("Accept UDP requests"). We send nothing in the first phases, so we need no setting changed beyond the UDP address.
- **No message can change dial frequency or band** in any stock release through 3.2.0-rc1. Switching named configurations restarts the main window and is unsuitable.
- **WSJT-X 3.0 has built-in FT8 band hopping**: it cycles through ticked bands or up to eight dial frequencies, and stops while Tx is enabled.

### When WSJT-X is not running

Heartbeats stop (or a Close arrives). The provider reports "no decoder" within about 30 seconds, observations go stale visibly, and prediction continues unaffected.

### Version and fork compatibility

- JTDX: compatible for the messages above; its Status message has a different tail. Last full release 2022.
- WS (formerly WSJT-X Improved): same protocol; identifies itself as `WS`.
- MSHV: reported compatible (unverified).
- JS8Call: different API; out of scope.

### Message parsing notes

- The sender is the second callsign in a standard message; the grid, when present, belongs to the sender.
- `RR73` matches the grid pattern and must be excluded.
- Stations that never send a grid are located by remembering their grid from earlier messages, or coarsely by callsign prefix. The source of each location is stored.

---

## 7. GridTracker-style integration model

GridTracker2 (BSD-3-Clause, Electron, active) is the reference for this pattern:

- It listens on UDP 2237, optionally joins a multicast group, and can forward the stream to another port (default 2238).
- It never decodes audio and never opens the radio.

We adopt the same model. WSJT-X decodes. Our app listens, stores, analyses, maps and compares with prediction. This avoids duplicating a mature decoder and carries no licence obligation.

**Second passive source.** WSJT-X's `ALL.TXT` log includes date, time, dial frequency (to 1 kHz), SNR, DT, DF and message. It serves as an import format for history, and as a fallback where UDP is blocked. Checked 2026-10-10 against a real 55,229-line log spanning December 2025 to September 2026: every line parsed, 90% of received decodes carried or recalled a locator, and the only messages without a sender were unresolved hashed calls (`K0RAR <...> +05`), free text and contest exchanges. The same decode arriving over UDP and from the log is stored once.

---

## 8. Coexistence with GridTracker, JTAlert and loggers

- **Unicast UDP has one listener per port.** If WSJT-X sends to `127.0.0.1:2237`, only one program receives it.
- **Multicast is the intended sharing mechanism.** Set WSJT-X's UDP server address to a multicast group. Every listener joins it. Since WSJT-X 2.3 multicast goes out on the loopback interface only by default.
- Addresses in use: the WSJT-X notes call `224.0.0.1` a safe choice; JTAlert recommends `224.0.0.123` since version 2.81.0; GridTracker2's documentation suggests `224.0.0.73` across machines.
- **Our listener (proposal):** try, in order, the configured multicast group, a forwarded port from GridTracker2 or JTAlert, then plain unicast. Bind with address reuse. Never send. Show which mode is active.
- **N1MM+** documents multicast sharing and advises starting WSJT-X first.
- **Linux** may need multicast enabled on the loopback interface (unverified). macOS behaviour is unverified and goes on the test list.
- A setup page in the app will show the exact WSJT-X settings and test reception.

---

## 9. Hamlib and rigctld architecture

### Facts

- Hamlib stable is 4.7.2 (June 2026). It fixes two `rigctld` security bugs; older versions should not be shipped.
- `rigctld` is a TCP daemon (default port 4532) with a simple text protocol. It binds to all interfaces by default; we start it bound to `127.0.0.1`.
- It supports several clients at once: one thread per client, each command atomic. Sequences from different clients can interleave, and its own manual says multi-client sharing needs more development.
- It caches rig state for 1 second by default, so read-backs can be stale. The cache time-out is adjustable.
- The `--vfo` option changes the command syntax for every client. All clients must agree.
- There is no read-only client mode and no working password. PTT can be disabled for the whole daemon with PTT type "none", which also stops WSJT-X keying through it.
- Errors return as `RPRT -n` (time-out −5, I/O −6, rejected −9, and so on).
- A dummy rig (model 1) and about 60 protocol simulators exist for testing.
- WSJT-X installers include `rigctld-wsjtx` specifically so that other applications can share the CAT connection.

### Options compared

| Option | Cross-platform | Multi-client | Notes |
|---|---|---|---|
| `rigctld` | Yes | Yes | The server WSJT-X documents and ships. **Recommended.** |
| flrig (XML-RPC) | Yes | Yes (unverified) | Good second backend; Hamlib can also talk to it |
| OmniRig, DX Lab Commander | Windows only | Yes | Not suitable as the base |
| Direct serial CAT | Yes | **No** | The port is exclusive. Only for stations not running WSJT-X. |
| Serial port splitters | — | — | WSJT-X states these are unsupported |

### Proposed `RadioController`

- First backend: a `rigctld` TCP client. Second: flrig.
- The interface has **no transmit function at all**. It can read frequency, mode, PTT, split and VFO, and set frequency.
- The app can start `rigctld` itself, or attach to one already running.

---

## 10. WSJT-X and CAT coordination

### The four architectures

| | Description | Assessment |
|---|---|---|
| **A** | We own the radio; WSJT-X has no rig control | WSJT-X no longer knows the dial frequency. Its Status messages and any PSK Reporter spots would carry the wrong band. **Rejected**, unless spotting is off, and even then fragile. |
| **B** | We ask WSJT-X to change band | **Not possible.** No UDP message sets frequency. Configuration switching restarts the window. |
| **C** | Both are clients of one `rigctld` | **Workable and the recommended end state.** WSJT-X polls the rig every 500 ms, adopts externally made frequency changes when not transmitting, reports them in Status, and does not re-assert its own frequency during receive. |
| **D** | Observe only | **The MVP.** No CAT at all. |

There is also **D+**: the operator enables WSJT-X 3.0's own FT8 band hopping, using a hop list our app recommends from the prediction. WSJT-X keeps sole control of the radio. This delivers a first prediction-guided scan with no CAT code.

### Recommended sequence

D, then D+, then C.

### Rules for architecture C (proposal)

WSJT-X must be set to "Hamlib NET rigctl". Before each scan the app checks and refuses to start if any rule fails:

1. **Never transmit.** The controller cannot send PTT. Before every retune it reads PTT from the rig and the Transmitting and Tx Enabled flags from WSJT-X. If Tx is enabled the scanner pauses; the operator is working stations.
2. **Split must be off.** WSJT-X split mode "Rig" programs the transmit VFO and turns split on while monitoring. After our retune the transmit VFO could be on another band. Scanning requires WSJT-X split set to "Fake It" or "None", and split reading "off" at the rig.
3. **Frequency only.** We never set mode. FT8 uses the same mode on every band. After each retune we read back frequency and mode, because a band change can recall a different stored mode on some radios (Hamlib issues a band-select on Yaesu radios). A mismatch stops the scan.
4. **WSJT-X's "Monitor returns to last used frequency" must be off**, or toggling Monitor will pull the rig back.
5. **Save and restore.** Frequency, mode, VFO and split are saved before a scan and restored on stop, on error and on exit.
6. **One writer.** Only the scheduler issues retunes, one at a time. Read-backs allow for the 1-second cache.
7. **Stop is immediate.** A permanent STOP SCAN control, and any CAT error, end the scan and restore state. WSJT-X takes a CAT error as "rig offline", so our client must never leave `rigctld` hung.
8. **Hardware warning.** Automatic band changes can make external tuners and amplifiers follow. The first-run check asks the operator to confirm the antenna system is safe to retune on receive.

---

## 11. Adaptive prediction-guided scanning

### FT8 timing facts

- 15-second slots. A transmission starts 0.5 s into the slot and lasts 12.64 s, leaving about 1.9 s.
- WSJT-X 3.0 runs decode passes at about 11.8, 13.5 and 14.4 s; results from the last pass arrive just after the slot ends. The Status message's Decoding flag shows when decoding finishes.
- Stations transmit in alternate slots, so hearing both sides of activity needs at least two consecutive slots.
- Decode threshold is about −21 dB in 2500 Hz.

### Cost of a hop (derived)

One slot is degraded per band change, whichever way the retune is timed:

- Retune after the previous slot's decoding completes (about 1–3 s into the next slot): the previous slot is complete, the slot in progress is a partial "settling" slot.
- Retune in the 1.9 s gap before the slot boundary: the next slot is clean, but WSJT-X 3.0 withholds the previous slot's later decode passes.

Proposal: start with the first, mark the settling slot, keep its decodes as evidence, and exclude it from rate statistics. Test the second with real hardware.

**So a dwell costs (useful slots + 1) × 15 s.**

| Dwell | Useful slots | Total time |
|---|---|---|
| Probe | 2 | 45 s |
| Standard | 4 | 75 s |
| Long | 8 | 2 min 15 s |
| Deep | 20 | 5 min 15 s |

These are starting values, to be tuned by replaying recorded sessions.

### Priority (proposal, deliberately simple)

For each enabled band:

`priority = prediction + recent observation + staleness + change + operator weight`

- **Prediction.** For a chosen destination: predicted reliability and SNR on that path. With no destination: predicted reach, meaning the share of reference areas where an FT8 signal should be decodable.
- **Recent observation.** The band's last observed tier, decaying with age.
- **Staleness.** Grows with time since the band was last sampled.
- **Change.** A boost when the last sample differed sharply from the one before.
- **Operator weight.** Pinned or excluded bands, and bands the antenna cannot use.

Bands are placed in three tiers by priority: long dwell, standard dwell, probe.

### Guarantees

- **Probe floor.** Every enabled band is sampled at least once per maximum interval (starting value 20 minutes), whatever its priority. This is a hard rule, not a weight.
- **Surprise promotion.** A band predicted poor that yields several distant stations in a probe is promoted for the next few cycles and flagged INVESTIGATE.
- **Quiet demotion.** A band predicted good that stays silent over consecutive dwells drops one tier but never below probe, and is labelled "predicted open, little heard", not "closed".

### Example cycle

Three top bands at long dwell, two middle bands at standard dwell and two probes: about 11 minutes. Lower bands rotate through the probe positions.

### Scan modes

| Mode | Behaviour |
|---|---|
| Prediction-Guided (default) | As above |
| Quick | One probe on every enabled band |
| Standard | Standard dwell on each selected band |
| Deep | Deep dwell on selected bands |
| Fixed Band Monitor | No retuning; identical to observe-only |
| Custom | Operator picks bands and dwell times |

### Scheduler design

The scheduler is a pure function of predictions, observation history, clock and settings. That makes it testable by simulation without a radio.

### Phase 7 results (2026-10-10)

Shipped as the Plan tab, without radio control. `scan::plan` is the pure scheduler: priority = FT8 prediction (reliability to a destination, or the share of the world in reach at the hour) + a bonus for what was heard in the last hour (0.5 strong, 0.3 moderate, 0.1 limited) + staleness (0 just listened, 1 after an hour or never). The top three bands get long dwells, the next two standard, the rest probes, with the dwell lengths from the table above; one pass over nine bands is 12 min 15 s, inside the 20-minute probe floor, and the plan repeats passes for the requested length. The tab lists the bands to tick in WSJT-X's band hopping in priority order, and a schedule with a follow mode that shows the band to be on, the time left and whether WSJT-X is on it.

Facts found while building it: WSJT-X's band hopping (stock and WSJT-X improved) only takes a set of bands or up to eight dial frequencies, hops on its own rhythm (WSJT-X improved: every other minute) and stops while Tx is enabled; there is no dwell setting, so a plan can choose its bands but not its timing until the app moves the radio itself (Phase 9). FT2 is not in stock WSJT-X 3.0: it is in WSJT-X improved 3.1.0 (open source, T/R period 3.75 s, half of FT4's) and, incompatibly, in the Decodium fork; its decode threshold is reported third-hand at about −12 dB (unverified) and it is said to need the clock within tens of milliseconds. Transmit-period counts now use 3.75 s for FT2, and the clock warning is shown on every screen.

---

## 12. Audio and clock

### Audio

- **None needed while WSJT-X decodes.** The app never opens a sound device in the first nine phases.
- If a native decoder is ever added: decoders expect 12 kHz mono; capture at 48 kHz and decimate. Candidate capture library for a Rust core is `cpal` (Apache-2.0).
- Two programs can share one input on Windows (shared mode), macOS, and Linux with PipeWire or PulseAudio. Raw ALSA devices are single-open.

### Native decoder options, for the record

| Option | Licence | Sensitivity | Assessment |
|---|---|---|---|
| WSJT-X UDP | none imposed | Reference | **Chosen** |
| `jt9` command-line decoder from a WSJT-X install | GPLv3, separate process | Same as WSJT-X | Best later option: spawn the user's installed copy |
| `ft8_lib` | MIT | About 73% of WSJT-X decodes in one benchmark (unverified) | Embeddable under any licence |
| PyFT8, Rust FT8 crates | GPL-3 | 77–89% (self-reported) | Would force GPL on our app if linked |
| Own decoder | — | — | No engineering reason |

All of these sit behind the `ObservationProvider` interface: `WsjtxUdpProvider`, `AllTxtImportProvider`, later `NativeFt8Provider` and `ExternalNetworkProvider`.

### Clock

- WSJT-X asks for UTC within ±1 s; the decoder searches ±2.5 s.
- **Offline self-check (proposal):** every decode carries DT, its time offset against our clock. Most stations are well synchronised, so the median DT over recent decodes estimates our own clock error. Existing tools (JTSync, jtxsync) use the same idea. Warn at about 0.5 s and alarm at about 1 s.
- **No decodes at all on a band predicted busy** is shown as "check clock and audio", not "band closed".
- **Sync sources, none mandatory:**
  - NTP when online.
  - GPS without PPS is good to a few hundred milliseconds, enough for FT8 with some margin.
  - WWV/CHU by ear.
- **Reading sync state without admin rights:**
  - Linux: `chronyc tracking`.
  - Windows: `w32tm /stripchart`.
  - macOS: `sntp` (unverified).
- **The app does not set the system clock** (it needs elevated rights). It reports the offset and tells the operator what to do.

---

## 13. Local observation database, metrics and recommendations

### Storage

SQLite in the user data directory, with versioned migrations and an automatic backup before each migration.

| Table | Contents |
|---|---|
| `observations` | UTC time, slot start, band, dial Hz, audio offset, SNR, DT, mode, raw message, message type, sender, addressee, CQ flag, grid, grid source (message, remembered, prefix), distance, bearing, country, **origin (LOCAL or EXTERNAL and which network)**, provider, flags (low confidence, settling slot) |
| `listening_intervals` | Band, dial frequency, start, end, slots heard, provider, scan ID |
| `predictions` | Input hash, engine and version, sunspot value and its source, outputs |
| `space_weather` | Parameter, value, issue time, fetch time, source, transport (bundled, Internet, Winlink, manual) |
| `stations`, `locations`, `scans`, `settings` | Profiles and configuration |

`listening_intervals` is essential. Without a record of when we were listening, "no decodes" cannot be told apart from "not listening".

A busy band can give 30–50 decodes per slot, so continuous monitoring may reach a few hundred thousand rows a day (estimate). Raw rows are rolled up into hourly band statistics and pruned after a configurable period.

### Metrics per band and time window

- Slots listened, decodes, decodes per slot.
- Unique callsigns, unique grids.
- Median and 90th-percentile SNR.
- Median and maximum distance.
- Count beyond 3,000 km.
- Counts by distance band.
- Occupied azimuth sectors.
- Change since the previous window.

### Terminology

The app says Observed Activity, Observed Reach, Decode Density, Median Decode SNR, Geographic Spread and Propagation Evidence. It does not say "band open" or "band closed" from FT8 data alone. Every observed figure shows how many slots it rests on.

Two permanent caveats appear in the interface:

- **No FT8 signals heard does not mean the band is closed.** Activity depends on who is on the air, their power and antennas, our noise, QRM and the decoder.
- **Hearing a station does not mean it can hear us**, and many FT8 signals do not imply a workable SSB path. FT8 decodes at about 13 dB-Hz; SSB needs 38–45 dB-Hz, roughly 25–30 dB more signal.

### Observed tier (proposal)

Four tiers instead of an opaque score: NONE, LIMITED, MODERATE, STRONG, from unique senders per listened slot and reach. Thresholds start as fixed provisional values and are later replaced by "relative to what this station usually hears on this band at this hour" once history exists.

### Comparing with prediction

- For comparison with FT8, VOACAP is run with a required SNR near 13 dB-Hz (the FT8 threshold of −21 dB in 2500 Hz, converted to 1 Hz; derived).
- The remote stations' power and antennas are unknown, so a typical station is assumed and the comparison is qualitative.
- **Path evidence:** for a chosen destination, stations heard near it, or along the same bearing at similar or greater distance, count as evidence for that path.

### Combined recommendation: a rule table, not a model

| Prediction | Observation | Label |
|---|---|---|
| Good | Strong or moderate | HIGH PRIORITY — predicted and confirmed |
| Good | Limited or none, adequately sampled | TRY — predicted open, little heard |
| Good | Not sampled or stale | PREDICTED — unconfirmed |
| Marginal | Strong | GOOD — better than predicted |
| Poor | Strong or moderate | INVESTIGATE — possible unexpected opening |
| Poor | None | LOW |

Each line shows its reasons (reliability, SNR, unique grids, median SNR, age of the data). Three rankings stay separately visible: Predicted, Observed, Combined. No machine learning.

### External networks (optional, online, late phase)

| Network | Access | Limits |
|---|---|---|
| PSK Reporter | HTTP query; MQTT feed | At most one query per 5 minutes; include a contact address |
| Reverse Beacon Network | Daily CSV archives; telnet feeds meant for cluster nodes | Share analyses |
| wspr.live | SQL over HTTP | Non-commercial; 20 requests per minute |
| WSPRnet | — | Policy not confirmed |

External rows carry `origin = EXTERNAL`, are never mixed into local metrics, and are shown with a distinct label and colour.

---

### First calibration against nine months of receptions (2026-10-10)

The first station's `ALL.TXT` (55,229 lines, December 2025 to September 2026; mostly 20 m, then 15, 10, 40 and 30 m) was run through `calibration::calibrate`: for every locator heard, the FT8 reliability VOACAP gives that path at that hour and month, with a 100 W isotropic station and residential noise assumed at both ends because the heard stations' equipment is unknown. 48,096 circuits took 143 s on the development PC. For each bin of predicted reliability the table gives the share of decodes that fell in it, and in how many of the hours the receiver was listening on that band a locator heard that month was actually heard.

| Predicted reliability | Share of decodes | Heard in listening hours |
|---|---|---|
| 0–10% | 5.0% | 17% |
| 10–30% | 4.2% | 14–22% |
| 30–50% | 5.7% | 27–30% |
| 50–70% | 16.0% | 34–35% |
| 70–90% | 22.5% | 38–42% |
| 90–100% | 46.8% | 45% |

On 20 m alone the curve runs from 9% to 39% without a step down. The model orders paths correctly. The absolute rate is bounded by whether the station was transmitting at all, so only the shape is read.

Two cautions. On 10 m and 15 m a fifth of all decodes came in hours the model put under 10%, and such hours still produced a decode six times in ten: the summer sporadic-E season (May was the busiest month in the log) is not in the model, and 0 dBi at both ends is pessimistic. This is exactly the case the Compare tab labels INVESTIGATE, and on the high bands in summer it is common, not rare. Second, the log records nothing in hours with no decode at all, so dead hours are missing from the listening count; that flattens the curve, and the true separation is somewhat larger.

Implications: the fixed reliability tiers (70% and 30%) are consistent with the data; the observed side is essential on the high bands; and a History screen can show this curve per band from the station's own database, which is the next step. The run is repeatable with `HFP_ALL_TXT=... cargo test calibration::tests::real_log -- --ignored --nocapture`.

## 14. UI and software architecture

### Stack recommendation: Tauri 2

- **Rust core** for the engine runner, UDP listener, CAT client, scheduler and database. **TypeScript UI** in the system web view.
- Small installers and low memory use suit field laptops and small ARM computers.
- Bundled helper executables ("sidecars") fit the engine subprocess model.
- A signed updater that works with GitHub Releases is part of the framework.
- Alternatives considered:
  - Electron: larger, proven by GridTracker2; a fair second choice.
  - Qt/C++: capable, slower to iterate.
  - Python with Qt: fastest for science code, hardest to package and update.
- Tauri and code-signing details here are from general knowledge and were not re-verified in this research pass.

### Modules and interfaces

```
PropagationEngine     predict(request) -> prediction        voacapl | hfcast | dvoacap | iturhfprop
SpaceWeatherProvider  latest() -> values with provenance    bundled | swpc | winlink | manual
ObservationProvider   stream of observations + status       wsjtx-udp | all-txt | native | external
RadioController       read state, set frequency, restore    rigctld | flrig          (no transmit)
ScanScheduler         next_action(state) -> dwell plan      pure, simulation-tested
ObservationStore      SQLite
WinlinkImporter       request text, folder watch, parse
```

Each module reports an explicit state (OK, degraded, stale, down) to a status bar. Nothing fails silently.

### Inputs for point-to-point prediction

- Maidenhead locator or latitude and longitude.
- Saved locations and station profiles.
- Our own grid from WSJT-X Status.
- GPS by serial NMEA or gpsd.
- Callsign lookup only when online.

### Station model

- Power, mode (sets required SNR), antennas from VOACAP's library, height, noise level, minimum takeoff angle.
- Verified reference values:
  - Noise: 145 residential, 155 quiet, 164 remote (−dBW/Hz at 3 MHz).
  - Required SNR: CW 19–24, SSB 38–45 dB-Hz.
  - Minimum angle: 3° with isotropic antennas.
- Presets: QRP portable, 100 W dipole, mobile, EmComm portable (NVIS), fixed gateway.
- Power comparison (5, 10, 50, 100 W) is a re-run per power level; runs are cheap.

### Screens

1. **Best Bands Now** — ranked bands with one-line reasons.
2. **Path** — hour-by-band chart of reliability and SNR, short and long path, MUF curve, day/night along the path.
3. **Compare** — the prediction-versus-observation table with disagreements highlighted.
4. **Map** — decoded stations, grids, great-circle paths, terminator, and predicted coverage underneath.
5. **Scanner** — current band, plan, STOP SCAN.
6. **Conditions** — every solar value with source, timestamp, age and current/stale flag, and whether predictions are climatological or adjusted.
7. **Field / EmComm mode** — one simplified screen: location, destination, recommended bands and time windows, observed evidence, data age, last scan.
8. **Advanced** — the raw engine deck and output for any prediction.

Maps are drawn from bundled vector data. No tile server is needed.

---

## 15. Validation strategy

| Area | Tests |
|---|---|
| VOACAP reference cases | Run a fixed set of decks through Windows `voacapw.exe` to produce golden outputs. Compare `voacapl` on each platform, and every candidate engine, field by field with stated tolerances. No official public test suite exists. |
| Engine comparisons | Same circuits through VOACAP and ITURHFProp; differences are reported, not failed. A few circuits compared by hand with VOACAP Online. |
| Path and environment | Data directory with spaces, long paths, non-ASCII user names, read-only install location. |
| WSJT-X parsing | Captured datagrams from WSJT-X 2.6, 2.7, 3.0.x, JTDX and WS as fixtures. Truncated packets, unknown types, extra trailing bytes. |
| Recorded sessions | A replay tool feeds recorded UDP sessions through the whole pipeline at any speed. |
| Message parsing | A corpus of real FT8 messages including compound calls, contest exchanges and `RR73`. |
| Observation import | `ALL.TXT` samples across versions; midnight roll-over. |
| CAT | `rigctld` with the dummy rig in CI. Fault injection: daemon killed, time-outs, PTT asserted mid-scan, split on, mode changed after retune, stale cache. |
| Scheduler | Deterministic simulation. Properties: every enabled band is probed within the maximum interval; no retune while PTT or Tx Enabled; state is always restored. |
| Offline | The full suite runs with networking blocked. |
| Stale data | Injected clock; age and stale labels at each threshold. |
| Clock | Synthetic DT offsets drive the warning states. |
| Comparison logic | Table-driven tests of the recommendation rules. |
| Space-weather parsers | Saved copies of each NOAA product, including missing-value markers and the 2026 JSON format change. |
| Upgrade | Install version N, create data, update to N+1, verify data and settings survive. |

---

## 16. Technical risks

| # | Risk | Mitigation |
|---|---|---|
| 1 | `voacapl` has never been built natively on Windows | **Resolved in Phase 0**: it builds and matches the Linux build exactly. |
| 2 | Engine path limits collide with default data directories | **Mostly resolved in Phase 0**: spaces work; the engine runs from its own folder with a relative data path. A run folder path beyond about 128 characters still fails, with a clear error. |
| 3 | Sunspot scale mismatch (about one band of error) | Explicit setting; documented default; later informed by our own history. |
| 4 | A third-party engine (`hfcast`, DVOACAP) differs from VOACAP | Golden-case harness decides; engine name shown with every prediction. |
| 5 | WSJT-X protocol drift (new types in 3.2) and version differences in decode timing | Tolerant parser; fixtures per version; our own dial-change guard. |
| 6 | Shared `rigctld`: interleaving, cache staleness, `--vfo` mismatch, band-stack mode recall, split hazard | Pre-flight checks and read-backs (section 10); CAT is a late phase. |
| 7 | Retune side effects on tuners and amplifiers | Explicit operator confirmation; receive-only; easy STOP. |
| 8 | Winlink catalog items can change or return an unexpected product (`PROP_RSGA` returns the SGAS product) | Content-based parsers, tested against real replies. |
| 9 | NOAA endpoints change (they did in March 2026) | Versioned, tolerant parsers; saved fixtures; failure is visible, not silent. |
| 10 | FT8 evidence is biased by who is on the air | Terminology, slot counts, baselines; never "closed". |
| 11 | GPL code entering the build by accident | Section 3 list; CI licence check; GPL programs only as separate processes. |
| 12 | Unsigned installers trigger Windows and macOS warnings | Accepted for now; signing deferred (section 19). Install instructions will explain the warnings. |
| 13 | Windows web view missing on older offline machines | Ship the offline web-view installer inside ours. |
| 14 | `rigctld` security history | Bind to loopback; require 4.7.2 or later. |
| 15 | ITU licence wording too narrow to bundle ITURHFProp | Ask ITU-R SG3; keep it optional. |

---

## 17. GitHub distribution and upgrades

- **Repository.** Public on GitHub. Issues and Discussions for user feedback.
- **Builds.** GitHub Actions on every tag, for Windows x64, Linux x64 and arm64, macOS Apple Silicon and Intel. The same workflow compiles the engine for each target and runs the reference cases before packaging.
- **Releases.** Installers attached to GitHub Releases, with semantic versions, a changelog, and stable and beta channels.
- **In-app update.** When online, the app checks GitHub Releases, verifies the update's signature against a key built into the app, and installs on request. The check never blocks start-up and can be turned off.
- **Offline update.** The same installer files can be carried on a USB stick and run directly. An "Install update from file" action verifies the signature first.
- **Data updates separate from app updates.** The sunspot table, prefix table and map data are versioned "data packs". They update online, from a file, or (for the sunspot pack) over Winlink.
- **Upgrades keep user data.** Settings and the database live in the user data directory. Migrations run on first start after an update, after an automatic backup. A downgrade refuses to open a newer database and says so.
- **Third-party notices** are generated at build time and shown in the About screen.

---

## 18. Implementation roadmap

Changes from the proposed order, and why:

- **Phase 0 added.** The Windows engine build is the largest unknown and decides the architecture. The release and update pipeline also goes in at the start, so every later phase ships as an ordinary update.
- **Winlink import moved from Phase 9 to Phase 3.** It uses the same parsers as the Internet path and is central to the no-Internet goal.
- **Combined recommendations moved ahead of CAT.** They need no radio control and deliver most of the value with none of the risk.
- **A no-CAT scanning step added** using WSJT-X's own band hopping.

| Phase | Deliverable | Exit test |
|---|---|---|
| 0 | Engine spike and scaffold: build `voacapl` on all three systems; golden-case harness; evaluate `hfcast-engine` and DVOACAP against it; skeleton app; CI; signed release that updates itself | Reference cases pass on all three systems, or a fallback is chosen with evidence |
| 1 | Offline point-to-point predictor: engine interface, locations, station profiles and presets, bundled sunspot table | Predictions with networking disabled match the reference cases |
| 2 | Best Bands Now, hour-by-band charts, short and long path, power comparison, coverage map | Operator questions in the brief answerable from the interface |
| 3 | Solar data: NOAA fetch and cache, age and staleness display, Winlink request text, paste and file import | Same product imported by Internet and by paste gives identical stored values |
| 4 | WSJT-X UDP observe-only: multicast listener, parser, observation store, listening intervals, clock-offset monitor, `ALL.TXT` import | Runs alongside GridTracker2 and JTAlert; replayed sessions reproduce stored observations |
| 5 | Observation analytics and map: metrics, station map, prediction overlay | Metrics verified against hand-counted recorded sessions |
| 6 | Prediction versus observation, combined recommendations, Field/EmComm mode | Rule-table tests pass; field screen usable offline |
| 7 | Scan assist without CAT: recommended hop list for WSJT-X's built-in band hopping; per-band dwell accounting | Multi-band comparison populated with no CAT code |
| 8 | Radio controller, read-only first: `rigctld` client, state display, save and restore, pre-flight checks | Dummy-rig and fault-injection tests pass |
| 9 | Prediction-guided automatic scanning through shared `rigctld` | Simulation properties hold; supervised on-air trial with two radio families |
| 10 | History and calibration: recurring openings, accuracy tracking, station baselines; Winlink folder watch; optional external networks, second engine, native decoder | — |

---

## 19. Decisions and open items

**Decided by the project owner (2026-10-04)**

1. **App licence: Apache-2.0.** It keeps a later move to GPL open if GPL decoder code is ever linked.
2. **Code signing: deferred.** No certificates for now, so early builds are unsigned and Windows and macOS will show install warnings. Update files are still signed with the project's own free updater key, which is separate from operating-system code signing.
3. **UI stack: Tauri 2.**
4. **Sunspot scale default:** current published (new-scale) values, as VOACAP Online does, with an advanced setting (section 4).
5. **First test radio: Yaesu FTDX10.** Its Hamlib backend is expected to be the Yaesu one that issues a band-select on band changes (unverified), so rule 3 in section 10 applies directly.

**To obtain**

- Copies of real Winlink replies for `wwv.txt`, `sgas.txt`, `3-day-forecast.txt` and `27-day-outlook.txt`, with callsign and message ID removed, as parser fixtures. Replies were received and inspected on 2026-10-04 but are not yet in the repository.
- UDP captures from a normal WSJT-X session, as parser fixtures.

**To verify**

- Multicast on macOS loopback.
- The flux-to-sunspot regression.
- Saildocs handling of a small text file from GitHub.
- Terms for the callsign prefix table.
- ITU-R's position on bundling ITURHFProp.

---

## 20. Sources

**VOACAP and engines**
- HFWin32: https://www.greg-hand.com/hfwin32.html · source: https://www.greg-hand.com/voacap_source · readme: https://www.greg-hand.com/pc_hf/readme.txt · command line: http://www.greg-hand.com/hf_news/command.txt
- NTIA/ITS: https://its.ntia.gov/software/high-frequency/voacap-propagation-model/ · https://its.ntia.gov/software/high-frequency/high-frequency-propagation-models/
- voacapl: https://github.com/jawatson/voacapl · man page: https://manpages.debian.org/trixie/voacapl/voacapl.1.en.html · issues 13 and 14 in that repository · Debian copyright: https://metadata.ftp-master.debian.org/changelogs/main/v/voacapl/voacapl_0.7.6-3_copyright
- Benchmark and usage notes: https://ionis-ai.com/tools/voacapl/
- VOACAP documentation: https://www.voacap.com/2023/voacapw-intro.html · https://www.voacap.com/2023/voacap-windows-output.html · https://www.voacap.com/2023/itshfbc-help/method30.html · https://www.voacap.com/2023/itshfbc-help/voacap-faq.html · https://www.voacap.com/2023/understanding/10mistakes.html · https://www.voacap.com/2023/understanding/choosingssn.html · https://www.voacap.com/2023/itshfbc-help/general.html
- VOACAP Online manual: https://www.voacap.com/2023/documents/VOACAP_Online_User_Manual_16_July_2024.pdf · terms: https://www.voacap.com/
- DVOACAP: https://github.com/VE3NEA/DVOACAP · dvoacap-python: https://github.com/skyelaird/dvoacap-python · hfcast-engine: https://github.com/jonathanmcsweet/hfcast-engine
- ITURHFProp: https://github.com/ITU-R-Study-Group-3/ITU-R-HF · P.533: https://www.itu.int/rec/R-REC-P.533/en
- Sunspot scale: https://www.sidc.be/SILSO/newdataset · https://k9la.us/Apr16_NEW_Sunspot_Numbers.pdf · effective sunspot number: https://hamwaves.com/voacap.ssn/en/ · https://spawx.nwra.com/spawx/ssne.html
- ICEPAC caution: https://www.voacap.com/2023/documents/GLane_ICEPAC.pdf · PyLap: https://github.com/HamSCI/PyLap

**WSJT-X and FT8**
- Protocol definition: https://github.com/WSJTX/wsjtx/blob/v3.0.2/Network/NetworkMessage.hpp
- Client behaviour: https://github.com/WSJTX/wsjtx/blob/v3.0.2/Network/MessageClient.cpp · https://github.com/WSJTX/wsjtx/blob/v3.0.2/widgets/mainwindow.cpp · https://github.com/WSJTX/wsjtx/blob/v3.0.2/Transceiver/PollingTransceiver.cpp
- User guide: https://wsjt.sourceforge.io/wsjtx-doc/wsjtx-main-3.0.0.html · release notes: https://wsjt.sourceforge.io/Release_Notes.txt
- FT8 parameters: https://github.com/WSJTX/wsjtx/blob/v3.0.2/lib/ft8/ft8_params.f90 · default frequencies: https://github.com/WSJTX/wsjtx/blob/v3.0.2/models/FrequencyList.cpp
- Multicast guidance: https://www.mail-archive.com/wsjt-devel@lists.sourceforge.net/msg19314.html · https://www.mail-archive.com/wsjt-devel@lists.sourceforge.net/msg21925.html
- GridTracker2: https://gitlab.com/gridtracker.org/gridtracker2 · https://docs.gridtracker.org/latest/GridTracker-Overview/Settings-Sub-Menus.html
- JTAlert: https://hamapps.com/php/notes.php?file=jtalert · N1MM+: https://n1mmwp.hamdocs.com/manual-windows/wsjt-x-decode-list-window/
- JTDX protocol: https://github.com/jtdx-project/jtdx/blob/master/NetworkMessage.hpp · WS: https://wsjt-x-improved.sourceforge.io/
- GPL FAQ on separate programs: https://www.gnu.org/licenses/gpl-faq.html#MereAggregation
- Independent clients: https://github.com/bmo/py-wsjtx · https://github.com/k0swe/wsjtx-go
- Decoders: https://github.com/kgoba/ft8_lib · https://github.com/G1OJS/PyFT8 · https://github.com/rtmrtmrtmrtm/ft8mon

**Hamlib, CAT, clock, audio**
- Releases: https://github.com/Hamlib/Hamlib/releases · news: https://raw.githubusercontent.com/Hamlib/Hamlib/master/NEWS
- rigctld manual: https://raw.githubusercontent.com/Hamlib/Hamlib/master/doc/man1/rigctld.1 · commands: https://www.mankier.com/1/rigctl
- Simulators: https://github.com/Hamlib/Hamlib/tree/master/simulators
- WSJT-X Hamlib client: https://raw.githubusercontent.com/WSJTX/wsjtx/master/Transceiver/HamlibTransceiver.cpp · split emulation: https://raw.githubusercontent.com/WSJTX/wsjtx/master/Transceiver/EmulateSplitTransceiver.cpp
- flrig: https://github.com/w1hkj/flrig · OmniRig: https://github.com/VE3NEA/OmniRig
- GPS time: https://gpsd.gitlab.io/gpsd/gpsd-time-service-howto.html · DT-based sync tools: http://www.dxshell.com/jtsync.html · https://github.com/CraigBladow/jtxsync
- Audio: https://github.com/RustAudio/cpal · https://learn.microsoft.com/en-us/windows/win32/coreaudio/exclusive-mode-streams

**Winlink and space weather**
- Winlink FAQ: https://winlink.org/sites/default/files/download/wl2k_faq_dec_1_2019.pdf · B2F format: https://winlink.org/B2F
- Catalog request walkthrough: https://groups.io/g/gaares/attachment/356/0/CatalogRequestExerciseCopyPaste.pdf
- Pat: https://github.com/la5nta/pat/wiki/The-command-line-interface
- Saildocs: https://saildocs.com/info · https://saildocs.com/terms
- NOAA SWPC data: https://services.swpc.noaa.gov/ (`/json/solar-cycle/`, `/products/`, `/text/`) · terms: https://www.weather.gov/disclaimer
- 2026 format change: https://www.weather.gov/media/notification/pdf_2026/scn26-21_Data_Format_Changes_Impacting_SWPC_Products.pdf
- SILSO terms: https://www.sidc.be/SILSO/datafiles · GFZ Kp: https://kp.gfz.de/en/data · GIRO terms: https://giro.uml.edu/didbase/RulesOfTheRoad.html · KC2G: https://prop.kc2g.com/about/
- PSK Reporter: https://pskreporter.info/pskdev.html · http://mqtt.pskreporter.info/ · RBN: https://www.reversebeacon.net/raw_data/ · wspr.live: https://wspr.live/
