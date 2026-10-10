# HF Predict manual

HF Predict tells you which HF bands should reach a place, and at what hours. It runs the VOACAP propagation model on your own computer, listens to what WSJT-X decodes, and sets the two side by side so you can see where the model and your receiver agree. It can also plan which bands to listen on and move your radio through that plan while WSJT-X listens.

The app is receive only. It has no transmit function, and the only things it ever changes on the radio are the frequency and the mode, during a scan you start yourself.

This manual describes version 0.13.0.

## Contents

1. [Installing](#installing)
2. [Your first prediction](#your-first-prediction)
3. [The main window](#the-main-window)
4. [Reading a prediction](#reading-a-prediction)
5. [Model views: Best bands, Through the day, Map, Engine](#model-views)
6. [Hearing with WSJT-X](#hearing-with-wsjt-x)
7. [Comparing the model with what you hear](#comparing-the-model-with-what-you-hear)
8. [Most contacts](#most-contacts)
9. [Space weather](#space-weather)
10. [Listening plans](#listening-plans)
11. [Connecting the radio](#connecting-the-radio)
12. [Scanning](#scanning)
13. [Stations, places and settings](#stations-places-and-settings)
14. [Where your data is kept](#where-your-data-is-kept)
15. [Troubleshooting](#troubleshooting)
16. [What the app will not do](#what-the-app-will-not-do)

## Installing

Download the installer for your computer from the [Releases page](https://github.com/KK4ODA/hf-predict/releases). There are builds for Windows, for macOS on Apple silicon, and for Linux as an AppImage, a `.deb` or an `.rpm` package.

The builds are not code-signed yet, so your system may warn you. On Windows, SmartScreen may say the publisher is unknown; choose *More info* and then *Run anyway*. If macOS refuses to open the app, allow it in System Settings under Privacy & Security.

Everything needed for a prediction comes with the app, including the VOACAP engine and a table of sunspot numbers, so it works with no Internet connection at all. The app goes online only to check for updates and when you press *Refresh from NOAA* on the Conditions view.

## Your first prediction

1. Type your position in **From**. You can use a Maidenhead locator of 4, 6 or 8 characters, such as `EM73tr`, or a latitude and longitude in decimal degrees, such as `33.7, -84.4`. South and west are negative numbers, or you can write the letters: `33.7N 84.4W`.
2. Type the other end of the path in **To** the same way.
3. Choose a **Mode**: SSB voice, CW or FT8.
4. Press **Predict**, or Ctrl+Enter.

The first result appears after a few seconds. The app remembers the path, the mode, the path direction and the view you were on, and predicts the same path again the next time it starts.

## The main window

![The Field view, with the status strip across the top, the path bar under it and the list of views on the left](screenshots/field.png)

The window has four parts: the status strip across the top, the path bar under it, the list of views on the left, and the view itself.

### The status strip

The strip shows the readings that matter on every screen. Most of them are buttons that open the view behind the reading.

| Reading | What it shows | Clicking it opens |
|---|---|---|
| Path | The two locators, the distance, the bearing, and whether you are looking at the short or long path | Puts the cursor in From |
| Time | The time in UTC and your local time | Nothing |
| SFI, A, K | Solar flux and the A and K indices from the latest WWV bulletin, with its age | Conditions |
| Receiver | The band and mode WSJT-X is on, *Waiting* when it has gone quiet, or *transmitting* | Heard |
| Radio | The radio's frequency and mode, or the band being scanned | Radio, or Plan while scanning |
| Clock | Appears only when the computer's clock looks wrong, with the offset in seconds | Heard |
| Update | Appears only when a newer version is available. Its dot pulses until you answer the update notice | The Updates section of Stations |
| Best now | The band the app recommends on this path at the current hour | Compare |

The dot beside Receiver and Radio is green when all is well, amber while it waits for something, red for an error or a keyed transmitter, blue while scanning, and hollow when the feature is off. When the window is narrow, the strip hides the less important details first.

Lines can appear under the path bar, on every view. A red line reports a geomagnetic storm, when the latest WWV bulletin gives a K index of 5 or more. An amber line reports that the computer's clock looks off, worked out from the timing of recent decodes. A grey line announces a new version, with buttons to install it or put it off; see [Updates](#updates).

### The path bar

The path bar holds everything that defines a prediction:

- **From** and **To**, with the ⇄ button between them to swap the two ends. Places you have saved appear as suggestions while you type.
- **Mode**, which sets how strong a signal must be. See [Reading a prediction](#reading-a-prediction).
- **Path**, *Short* or *Long*. Both directions are computed together, so switching changes the views at once.
- **Hour shown**, the UTC hour every view displays, with your local time beside it. The ‹ and › buttons step one hour; *Now* returns to the current hour.
- **More settings**, which opens a second row: the year and month to predict (the current ones to start with), *Required reliability %* (90 to start with), *Sunspot number* (leave it blank to use the table that comes with the app, or type a number to override it), and the station profiles for *My station* and *Other station*.
- **Predict**. When you change a setting after a prediction, the button changes to **Update** and shows a small dot; press it to predict again.

The app starts with the first station preset each time. If you have saved your own station, pick it under *More settings*.

### The views

| Group | Views |
|---|---|
| Operate | Field, Most contacts, Plan, Radio |
| Model | Best bands, Through the day, Map |
| Observe | Heard, Compare, History |
| Space weather | Conditions |
| Setup | Stations, Engine |

Views that need a predicted path show the word *path* beside their name until you predict one. A half-filled circle beside Conditions means some of the solar data is old or missing.

### Keyboard shortcuts

| Keys | Action |
|---|---|
| Ctrl+Enter | Predict |
| Alt+1 to Alt+9, then Alt+0 | Open the first ten views in list order, from Field to History |
| `[` and `]` | Previous and next hour |
| `n` | Back to the current hour |

The hour keys are ignored while you are typing in a box.

## Reading a prediction

VOACAP predicts monthly averages. For each band and hour it estimates the share of days in the month on which your signal reaches the signal-to-noise ratio your mode needs. HF Predict calls that share the *reliability*.

Each mode needs a different signal-to-noise ratio, measured in a 1 Hz bandwidth (dB-Hz):

| Mode | Needs |
|---|---|
| SSB voice | 38 dB-Hz |
| CW | 24 dB-Hz |
| FT8 | 13 dB-Hz, which is FT8's decode threshold of −21 dB in 2500 Hz |

Reliability is described in four steps. Each has a symbol, so you can tell them apart without colour.

| Symbol | Outlook | Reliability |
|---|---|---|
| `●` | Good | 70% of days or more |
| `◐` | Fair | 30 to 69% |
| `○` | Poor | 10 to 29% |
| `–` | Unlikely | under 10% |

A band at Fair or better is counted as worth trying. The colour scale under each chart shows how the shading maps to reliability.

A prediction describes a typical day of the month. It cannot know about today's storm or flare. The Conditions view shows the current readings so you can judge how today compares, and the app warns you when a storm is under way.

The model covers 2 to 30 MHz, so the bands are 80, 60, 40, 30, 20, 17, 15, 12 and 10 m. 160 m is below its range.

## Model views

### Best bands

![Best bands: every band ranked for the hour, with a 24-hour strip for each](screenshots/bands.png)

Best bands ranks every band for the hour shown and opens with a sentence naming the band to try first. Each row has:

- the outlook and a reliability bar, with ticks at 30% and 70%;
- the median signal-to-noise ratio, and how many dB it has to spare or falls short of what your mode needs;
- a 24-hour strip of reliability from 00 to 23 UTC, where the outlined hour is the one shown and a notch marks the current hour;
- the hours worth trying, in UTC and local time;
- the propagation mode the engine found, such as 2F2, and the take-off angle.

When the other path, long or short, is at least 10 points better at that hour, the row says so.

Under the table, *Effect of transmit power* gives each band's reliability at 5, 10, 50 and 100 W for the hour shown, with everything else unchanged. Each tenfold increase in power adds 10 dB of signal.

### Through the day

![Through the day: every band against every hour, with the MUF, FOT and LUF lines](screenshots/day.png)

This chart sets every band against every hour of the day, shaded by reliability. Three lines run across it:

| Line | Meaning |
|---|---|
| MUF | Maximum usable frequency, reached on half of the days |
| FOT | Optimum working frequency, reached on 90% of the days |
| LUF (dotted) | Lowest usable frequency for your mode |

Bands between the LUF and the FOT are the dependable choices. Hover over a cell to see its signal-to-noise ratio, the share of days the band is open, the propagation mode and the MUF. Click a column to make that hour the hour shown everywhere in the app.

*Numbers in cells* writes the reliability, the signal-to-noise ratio or the share of days open into each cell. *Show as a table*, under the chart, gives the same figures as numbers, with local time and the three frequencies for each hour.

### Map

![The map, with predicted coverage on 20 m, stations heard, the path and the night side](screenshots/map.png)

The map does not need a prediction. It shows the path, the night side of the earth for the hour shown at mid-month, and a grid of Maidenhead fields. Drag to pan and scroll to zoom, or use the zoom buttons; the third button returns to the whole world.

- **Set From on map** and **Set To on map**: press one, then click the map. The position is entered as latitude and longitude. Press the button again to cancel.
- **Band** chooses the band for the two layers below. It starts at 20 m.
- **Show predicted coverage** shades the map with the predicted reliability of reaching a station like your *Other station* from the From position, on that band at the hour shown, with the antennas aimed to within 22.5° of each cell. The shading is cleared when you change the position, the hour, the stations or the mode; press *Recompute coverage* to draw it again.
- The last list adds a dot for each station decoded on the band in the last 15 minutes, hour, 6 hours or 24 hours, placed at the centre of the locator it sent. Choose *no heard stations* to hide them.
- *Stations hearing your area* adds an amber ring for each station heard, in the same span, sending a signal report to you or to a station near you. See [Who hears your area](#who-hears-your-area).

The dots show what your receiver heard, which depends on who was transmitting. The rings show who hears your part of the world. A dot inside a ring was heard both ways. The shading predicts where a signal from you would be heard.

### Engine

The Engine view shows the exact VOACAP run behind the prediction on screen: the engine version, the path with its bearings out and back, the sunspot number used and where it came from, and the required signal-to-noise ratio. The full input deck and output are underneath, each with a button to copy it, so you can check a result against another VOACAP program.

## Hearing with WSJT-X

HF Predict listens to the messages WSJT-X sends over the network after each decode. It never sends anything to WSJT-X. It works with WSJT-X and with WSJT-X improved.

### Setting up WSJT-X

1. In WSJT-X open *File*, *Settings*, *Reporting*. Under *UDP Server* set the address to `224.0.0.1` and the port to `2237`, and tick the loopback interface under *Outgoing interfaces*.
2. In HF Predict open Heard, enter the same address and port, tick *Listen for WSJT-X* and press *Apply*.
3. Set GridTracker, JTAlert and any other program that listens to WSJT-X to the same address and port.

An address from 224 to 239 is a multicast group, which any number of programs can share; each one receives every message. An ordinary address such as `127.0.0.1` reaches one program only. Use it only if nothing else listens on that port, or point HF Predict at a port another program forwards to (GridTracker forwards to 2238).

### The Heard view

![Heard: the listener, activity by band and the latest decodes](screenshots/heard.png)

The top panel says whether the app is receiving. For each copy of WSJT-X it hears, it shows the program's name and version, its dial frequency, band and mode, your callsign and locator, and how long ago its last message arrived.

The **clock check** under it works out the median time offset (DT) of recent decodes. FT8 needs the computer's clock within about a second, and FT4 and FT2 need it tighter still. When the offset is too large, a warning appears on every view until you correct the clock.

**Activity by band** covers the last 15 minutes, hour, 6 hours or 24 hours. For each band it gives how long you listened, the number of decodes and decodes per transmit period, the callsigns and locators heard, the median signal-to-noise ratio and the level 90% of decodes stayed below, the median and farthest distance, the number of stations over 3000 km, and a direction rose drawn north up. The small *was* figure beside the callsigns is the count for the same length of time just before, for comparison.

**Latest decodes** lists the 200 most recent decodes. You can filter by band, search for a callsign or locator, and sort by time, signal or distance. In the list:

- an asterisk after a locator means the station sent none in that message, so the locator from its earlier message is used;
- grey rows were decoded while the receiver was changing frequency, and are left out of all counts;
- *live* decodes came over the network from WSJT-X, and *log* decodes were read from an ALL.TXT file.

Nothing heard on a band can simply mean nobody was transmitting, and hearing a station does not mean it can hear you.

### Who hears your area

Everything else on Heard is one direction: what your receiver hears. FT8 messages also carry the other direction. When a distant station sends a signal report, such as `N4NB G4AAA -07`, it is saying how well it hears that station. If that station is you, or is near you, the report tells you how well the distant station hears your area.

*Who hears your area* lists the distant stations heard sending such reports over the last hour, 6 hours, 24 hours or 7 days. For each it gives the time of its latest report, the band, its locator, distance and bearing from you, its best report, and whom it reported: *you*, or the nearby stations with how far they are from you.

- A report to your own callsign counts as hearing you. The app takes your callsign from WSJT-X and remembers it, so this works with logs read later too.
- A report to another station counts when that station is near you: within 300 km, or for a distant sender up to 15% of its distance, and never beyond 1,000 km. Seen from far away, a station a few hundred kilometres from you is in the same direction.
- A nearby station can only be placed once your receiver has heard it send its locator, at any time and on any band. Stations in your skip zone are often never heard, so their reports are missed.

The reporting station's signal and noise are not yours, and a nearby station may run more power than you. Treat a report as evidence that the path is open in your direction, and its value as a rough guide.

### Reading WSJT-X logs

Each WSJT-X installation keeps an ALL.TXT log of everything it decoded. If you have used more than one, such as WSJT-X and later WSJT-X improved, each has its own log in its own folder. Add all of them to bring your past decodes into the app.

- *Add log file…* lets you pick an ALL.TXT file.
- *Find logs* lists the logs in the usual places: folders whose names start with WSJT-X or JTDX, in `%LOCALAPPDATA%` on Windows, `~/.local/share` on Linux and `~/Library/Application Support` on macOS. Press *Add* beside each one you want.
- *Check now* reads any new lines straight away.

The app reads each log where it is, without copying it. It reads the logs when it starts and when you press *Check now*, and only reads lines it has not seen before. A log that has been replaced by a new file is read again from the start. Decodes that also arrived live over the network are stored once.

An ALL.TXT log does not record where the receiver was, so give each log a receiver position. It starts as your From position when you add the log. Distances and bearings are worked out from it. *Remove* takes a log off the list.

## Comparing the model with what you hear

### Compare

![Compare: the model's FT8 prediction beside the stations heard toward the destination](screenshots/compare.png)

Compare sets the model's prediction for each band at the hour shown beside the FT8 stations your receiver decoded toward the destination. Both sides are FT8, because the decodes are FT8; your own mode's prediction is shown as well when it is different.

A decoded station counts as toward the destination when it is within 1,500 km of the To position, or within 15° of the path's bearing and at least 60% of the way there. A band needs at least four transmit periods of listening before it counts as listened to. One or two stations heard that way is limited evidence, three is moderate and eight is strong.

The controls choose how far back to look (15 minutes, an hour or 6 hours) and how to order the bands: by recommendation, by the model, or by what was heard. Each row gives:

- the recommendation, with a line of explanation;
- the model's bars;
- what was heard toward the destination, with up to three example callsigns and the best signal;
- *Hears your area*: how many stations toward the destination were heard reporting you or a station near you, how many of them reported you, example callsigns and the best report;
- how many callsigns were heard on the whole band. This column is hidden when the window is narrow.

The recommendation comes from a fixed table, so you can always trace it back:

| Symbol | Recommendation | Model says | Heard toward the destination |
|---|---|---|---|
| `●` | HIGH PRIORITY | Good, 70% or more | Moderate or strong |
| `●` | GOOD | Marginal, 30 to 69% | Strong |
| `▲` | INVESTIGATE | Poor, under 30% | Moderate or strong, a possible unexpected opening |
| `◐` | TRY | Good | Limited or nothing yet |
| `◐` | WORTH TRYING | Marginal | Moderate |
| `○` | PREDICTED | Good | Not listened to yet |
| `○` | MARGINAL | Marginal | Limited, nothing, or not listened to |
| `–` | LOW | Poor | Limited, nothing, or not listened to |

When your mode is SSB or CW and the band looks encouraging for FT8 but poor for your mode, the row adds a caution with the extra signal your mode needs. SSB needs about 25 dB more than FT8.

Hearing stations that way shows the band is open in that direction for FT8. It does not show that they can hear you, because their power and their noise may differ from yours. *Hears your area* is the evidence for the other direction, and it is shown beside the recommendation without changing it. The model's own figure is for your signal reaching the destination.

### Field

The Field view puts what you need for working a path on one screen, all from data on the computer. It names the three bands to try, best first, each with its recommendation, the model's figure, what was heard toward the destination, how many stations that way report hearing your area when there are any, a 24-hour strip and its good hours. When no band looks dependable at the hour shown, it says so; step the hour to find a better time.

Beside the bands are the map and a summary of how old each piece of data is: the solar readings, the sunspot number and table the prediction used, the last decode stored, and the span of listening counted (the last 60 minutes).

### Best now

*Best now* at the right of the status strip is the band Field would put first, worked out for the current hour whatever hour you are viewing. It needs a predicted path. Click it to open Compare.

### History

![History: how often places were heard, grouped by what the model predicted for them](screenshots/history.png)

History checks the model against every decode you have stored. Press *Check the model against my decodes*. For each decode with a locator, the app asks the model what it would have predicted for that place at that hour and month, then counts how often places were actually heard. The first run over a long log can take a few minutes, with a progress count; later runs reuse its predictions and are quicker.

The chart groups the places by what the model predicted for them, from under 10% on the left to 90% or more on the right. Each column shows how often you heard those places in the hours you were listening. The whisker is its 95% range, and the number under the column is how many listening hours it rests on. Columns resting on fewer than 20 hours are drawn faint. Choose a band to see that band alone.

If the model is any good, the columns rise from left to right. They stay well below the predicted figures, because a place is heard only when someone there is transmitting, so compare the shape of the columns and not their height.

The check places your receiver at the From position, uses the noise level of *My station*, and assumes the other station runs 100 W into a simple antenna. Decodes without a locator, on a band the model does not cover, or in a month outside the sunspot table are left out, and the count of them is shown. *Show as a table* gives the figures behind the chart.

## Most contacts

![Most contacts: every band ranked by how many of the stations in your log it should reach, and the whole day below](screenshots/contacts.png)

The other views are about one path. Most contacts is for when you want to work as many stations as you can, wherever they are. It does not need a To position.

For each band it adds up how many of the stations in your log a signal from you should reach. Each station is placed by the locator it last sent. The model gives the share of days your signal reaches that locator square in the mode chosen in the path bar, and those shares are summed over the stations. A band that reaches 90% of the days to 800 stations scores 720. The figure means "about this many stations, on a typical day of the month".

### Reading it

The view opens with the band to try first at the hour shown. The table ranks every band and gives:

- *Should reach*, the number of stations;
- *Share of those counted*, the same as a share of all the stations counted;
- the same number split by distance, from under 1,000 km to over 8,000 km. These four columns are hidden when the window is narrow;
- *Heard, last hour*: callsigns your receiver decoded on the band in the last hour;
- *Hear your area*: stations heard in the last hour reporting you or a station near you.

The last two columns are what is happening now, whatever hour is shown.

Two settings choose which stations count:

- *Count every station heard*, or only *stations usually on at this hour*: those heard within an hour either side of it, on any day and band. Where your log has fewer than 3 hours of listening around that time of day, it cannot tell who is usually on, so every station counts and the view says so.
- *From all of it*, or from the last year, 90 days or 30 days of your log.

### Through the day

*Work out every hour* fills a table of every band against every hour. Each cell holds the number of stations, shaded by the share of those counted. The hour shown is outlined, and clicking an hour shows it everywhere in the app. The first time for a month, station profile and mode takes about half a minute. After that the predictions are kept, and it takes about a second. The *Counted* row gives the number of stations behind each hour; an asterisk marks hours where only stations usually on at that hour were counted.

### What it leaves out

- The stations are the ones your receiver could hear. Places it never hears are missing, so the more you have listened, on more bands, the better the picture.
- Your log is of FT8 stations, even when the mode is SSB or CW. Voice operators are in broadly the same places, but not the same numbers.
- Each station is assumed to run a station like your *Other station* profile, with the antennas aimed at it.
- Stations within 100 km are left out, because the model covers sky wave only.
- A band that reaches many stations can also be crowded. Once plenty of stations are workable, the time each contact takes sets your rate, so reach beyond that adds little.

## Space weather

![Conditions: solar and geomagnetic readings with their source and age](screenshots/conditions.png)

The Conditions view collects solar and geomagnetic data with the source and age of each item. The model itself uses only one solar input, the monthly smoothed sunspot number from the table that comes with the app. Everything else here is for your own judgement.

### Getting the data

With an Internet connection, press **Refresh from NOAA**. The app fetches four products from NOAA's Space Weather Prediction Center and a fresh copy of the smoothed sunspot table, and lists what succeeded.

With no Internet, press **Request over Winlink**. The app shows a message to send from your Winlink program to `INQUIRY@winlink.org` with the subject `REQUEST`, one catalog item per line of the body. *Copy the request* copies it. When the replies arrive, use *Import files…* on the saved message files, or paste a reply into *Paste a product* and press *Import pasted text*. A NOAA product copied from anywhere else can be pasted the same way.

| Product | Winlink item | Counted as old after |
|---|---|---|
| Geophysical alert (WWV) | `PROP_WWV` | 6 hours |
| Daily solar and geophysical summary | `PROP_SGAS` | 36 hours |
| Three-day forecast | `PROP3DNOAA` | 1 day |
| 27-day outlook | `PROP.27DO` | 8 days |
| Smoothed sunspot table | from NOAA only | 45 days |

### What it shows

**Now** gives the solar flux, the A and K indices, the daily sunspot number and the X-ray background with any flares. The flux and the indices come with a few words saying what the value means, and every reading shows its source and age. **Next three days** gives the forecast Kp by period and the chances of radio blackouts and radiation storms. Under **Products**, *What the model uses* describes the sunspot table: when it was built, how far it is observed and predicted, and whether it came with the app or was downloaded. Each product can be opened to see when it was issued and how it was received.

The storm warning appears when a WWV bulletin less than 6 hours old reports a K index of 5 or more. An old bulletin raises no warning, since it says nothing about now.

To try a different solar input, type a sunspot number under *More settings* in the path bar. Leave it blank to go back to the table.

## Listening plans

![Plan: a listening plan to follow by hand, and the receive-only scan](screenshots/plan.png)

A listening plan says which bands to listen on, in what order and for how long, so that what you hear covers more than the band WSJT-X happens to be on. Nothing on this view transmits.

### Making a plan

1. Tick *Aim at the To position* to plan for your path, or clear it to plan for the whole world from your position.
2. Choose how long to plan for: 15 or 30 minutes, 1 hour or 2 hours.
3. Click a band to leave it out, and click it again to put it back.
4. Press *Make a plan*. The plan uses the hour shown in the path bar, so you can plan ahead for a later hour. A scan always plans for the current hour instead.

Each band gets a priority made of three parts: its FT8 prediction (to the To position, or the share of the world an FT8 signal should reach), a bonus of up to 0.5 for what was heard on it in the last hour, and up to 1 more for how long it has gone without being listened to, reaching 1 after an hour. The top three bands get long dwells of 8 transmit periods, the next two standard dwells of 4, and the rest a short probe of 2. Each dwell has one extra period for retuning, and a period is 15 seconds. Every band comes round at least once in 20 minutes however poor its prediction, so a surprise opening is not missed for long. *Why this order* shows the figures for each band.

### Following it by hand

*Listen by hand* lists the bands to tick in WSJT-X's band hopping, best first. WSJT-X hops on its own rhythm and stops while transmit is enabled; it cannot be told how long to stay on each band.

To follow the schedule yourself, press *Start following by hand*. The view shows the band to be on now, the time left and the band after it, and checks the band WSJT-X reports: a tick when it matches, or a reminder to switch. The schedule table adds the clock time each step starts. When the plan has run its course, make a new one to carry on.

To have the app move the radio instead, see [Scanning](#scanning).

## Connecting the radio

You need this only for scanning. HF Predict talks to the radio through Hamlib's `rigctld`, a small program that holds the radio's serial port and lets several programs share it. WSJT-X uses the same `rigctld`, so the two never fight over the port.

The app reads the radio's frequency, mode, transmit state (PTT), split and VFO every couple of seconds. Outside a scan it changes nothing.

### Setting up

1. Install Hamlib if you do not have `rigctld`. On some systems WSJT-X comes with a copy called `rigctld-wsjtx`.
2. On the Radio view, tick *Read the radio through rigctld*. The host `127.0.0.1`, port `4532` and reading every 2 seconds suit most stations.
3. Tick *Start rigctld for me*. Choose the `rigctld` program (the box suggests the ones it finds), your radio from the list, its serial port and its speed in baud.
4. If you like, tick *Start WSJT-X once rigctld is up* and choose the WSJT-X program. The app then starts WSJT-X itself, waiting up to 90 seconds for `rigctld` to answer first. If that program is already running, the app leaves it alone and says so under the setting, since WSJT-X refuses a second copy of itself.
5. Check *Mode while scanning*, described under [Scanning](#scanning).
6. Press *Apply*.
7. In WSJT-X's radio settings, set the rig to *Hamlib NET rigctl* with the network server `127.0.0.1:4532`, and keep the PTT method you used before.

WSJT-X looks for `rigctld` only when it starts. Start HF Predict first, or press *Retry* in WSJT-X's rig error once `rigctld` is up, or let HF Predict start WSJT-X for you.

If a `rigctld` already answers on the port, the app uses it and does not start a second one.

### The Radio view

![Radio: the radio read through a shared rigctld](screenshots/radio.png)

The readout shows the frequency, band, mode and passband, whether the radio is receiving or transmitting, the VFO and split. The WSJT-X line compares the radio's dial with the one WSJT-X reports; a difference means WSJT-X may be on the other VFO, or not using this `rigctld`.

Hamlib calls the data modes by its own names. The app shows them as radios do, so Hamlib's `PKTUSB` appears as DATA-U.

Under the settings, a line reports the connection, and when the app started `rigctld` it shows the exact command and process number. If `rigctld` stops, its exit code and last output appear there, and the app starts it again.

### Quitting with rigctld running

When the app started `rigctld`, closing the app asks what to do with it, because while it runs no other program can open the radio's serial port directly.

- *Stop rigctld* stops it. Choose this when you are finished with the radio, or another program needs the port.
- *Leave it running* keeps it going, for example because WSJT-X is still using it. The next time HF Predict starts, it takes that `rigctld` back instead of starting another.
- *Cancel* keeps the app open.

If a scan is running, it stops and the radio goes back first.

## Scanning

A scan moves the radio through a listening plan while WSJT-X decodes, so the picture on Compare and Field stays current on every band. The scan only receives.

### Before the first scan

In WSJT-X:

- set split operation to *None* or *Fake It*, because *Rig* split can leave the transmit VFO on another band;
- turn off *Monitor returns to last used frequency*, or toggling Monitor pulls the radio back;
- leave *Enable Tx* off while scanning.

On the Radio view, choose the **Mode while scanning**:

| Choice | What the scan does |
|---|---|
| DATA-U (Hamlib PKTUSB) | Keeps the radio in DATA-U. This is the default. |
| USB | Keeps the radio in USB. |
| Keep the mode the radio has when the scan starts | Keeps whatever mode you had. |

Some radios, the Yaesu FTDX10 among them, recall each band's last mode when the band changes. After every retune the scan reads the radio back, and if it came up in another mode, sets the chosen mode again.

### Starting and stopping

The *Scan automatically, receive only* panel on the Plan view lists anything that stands in the way. *Start scanning* is available only when it reads Ready, which needs:

- the radio connected, receiving, with split off;
- WSJT-X reporting over the network, with transmit disabled;
- a From position;
- the box *My antenna system (tuner, amplifier) is safe to retune on receive* ticked. Automatic band changes can make an external tuner or amplifier follow the radio.

*Make a new plan when this one ends* is ticked to begin with. With it ticked, the scan makes a fresh plan from the same settings each time one runs out, for the hour it is by then, and keeps going until you press **STOP SCAN**. Clear it to stop after one plan.

The scan uses the settings above it on the Plan view (aiming, length and bands). It always plans for the current UTC hour and month, whatever hour the path bar shows, because it listens now: the first plan is for the hour the scan starts in, and each new plan for the hour it begins in. It does not need a plan made first.

While it runs, the panel shows the band it is on and the next one, the number of retunes, the mode it holds the radio in and how often it had to set it back, how many plans it has renewed, and the frequency the radio will return to. The Radio reading in the status strip shows the band being scanned.

### Pauses and stops

The scan pauses, without retuning, while the radio transmits, while WSJT-X transmits or has *Enable Tx* on, and when WSJT-X has sent nothing for 30 seconds. It carries on by itself when the reason clears, starting with whichever band is due by then.

The scan stops when you press STOP SCAN, when split comes on at the radio, when the connection to the radio is lost, when the radio does not land on the frequency asked for, when it will not take the chosen mode, or when a new plan cannot be made. Whatever stops it, the app sets the radio back to the frequency, mode and passband it had when the scan began, and the panel says so. If that fails, the panel tells you what to set by hand.

Decodes made while the radio was changing band are kept, but shown grey on Heard and left out of the counts.

## Stations, places and settings

The Stations view holds your station profiles and the app's settings.

### Station profiles

There are two profiles: *My station, transmitting* and *Other station, receiving*. Each starts from a preset or a profile you saved, and has:

- the transmit power in watts;
- the lowest take-off angle in degrees;
- the antenna: a half-wave dipole 10 m or 5 m high, a quarter-wave vertical, a 2.5 m mobile whip, or an isotropic reference antenna;
- the local noise: residential, rural or remote.

Type a name and save the profile to use it again. Saved profiles appear in the *My station* and *Other station* lists under *More settings*.

The presets are:

| Preset | Power | Antenna | Noise |
|---|---|---|---|
| 100 W dipole | 100 W | Dipole, 10 m high | Residential |
| QRP portable | 5 W | Dipole, 5 m high | Rural |
| Mobile HF | 100 W | Mobile whip | Residential |
| EmComm portable | 100 W | Dipole, 5 m high | Rural |
| Fixed gateway (RMS) | 100 W | Dipole, 10 m high | Residential |

### Saved places

Give a place a name and a position, and press *Save place*. Saved places appear as suggestions in From and To, and the list has buttons to use a place as either end or remove it.

### Appearance

Choose *Dark* or *Daylight*. Daylight is easier to read outdoors and in bright rooms.

### Updates

The app looks for a newer release a few seconds after it starts and every six hours. When there is one, a line under the path bar says so, and *Update* appears in the status strip with a pulsing dot.

- *Install and restart* on that line downloads and installs the update, showing its progress, and restarts the app.
- *Later* hides the line and stops the pulse until the app next starts. *Update* stays in the status strip, and clicking it opens this section.

A running scan stops and the radio goes back before the update installs, and `rigctld` is left running for the new version to take back. *Check for updates* on this view looks straight away. Updates come from the project's GitHub releases and are signed.

### About and credits

The end of the Stations view credits the people and projects HF Predict is built on, VOACAP and the voacapl port above all, and holds the full notices and licences.

## Where your data is kept

Everything stays on your computer. The app sends nothing anywhere except its requests to NOAA, when you ask for them, and its check for updates on GitHub.

Saved stations, places and the list of logs are kept in `userdata.json`. Decodes are kept in `observations.db`, with the listener and radio settings, the solar data and the sunspot table in small files beside it.

| System | Saved stations and places | Decodes and everything else |
|---|---|---|
| Windows | `%APPDATA%\io.github.kk4oda.hfpredict` | `%LOCALAPPDATA%\io.github.kk4oda.hfpredict` |
| macOS | `~/Library/Application Support/io.github.kk4oda.hfpredict` | the same folder |
| Linux | `~/.config/io.github.kk4oda.hfpredict` | `~/.local/share/io.github.kk4oda.hfpredict` |

The ALL.TXT logs stay where WSJT-X keeps them.

## Troubleshooting

**The Receiver reading says Waiting.** WSJT-X is not reaching the app. Check that the address and port on Heard match WSJT-X's *Reporting* settings, and with a multicast address, that the loopback interface is ticked in WSJT-X. With an ordinary address, another program may already be using the port.

**A clock warning appears.** Synchronise the computer's clock with an Internet time server or a GPS receiver. FT8 decoding suffers when the clock is a second or more off.

**Radio shows Error.** Read the line under the Radio settings. A wrong serial port or speed is the usual cause. So is another program holding the port, such as WSJT-X set to talk to the radio directly instead of through *Hamlib NET rigctl*.

**WSJT-X reports a rig error when it starts.** It started before `rigctld` was up. Press *Retry* in WSJT-X, or tick *Start WSJT-X once rigctld is up* so HF Predict starts it in the right order.

**Start scanning is greyed out.** The checklist above the button names each thing that stands in the way.

**A scan stopped with "the radio is in USB after being set to DATA-U".** The radio did not accept the mode. Check that the radio model chosen on the Radio view is right, or choose USB or *Keep the mode the radio has when the scan starts*.

**WSJT-X was not started.** The line under *Start WSJT-X once rigctld is up* says why. If WSJT-X was already running, the app leaves it alone; if it shows a rig error, press *Retry* in WSJT-X.

**Another program cannot open the radio after HF Predict closed.** You chose *Leave it running* when you quit. Start HF Predict and quit again choosing *Stop rigctld*.

**The prediction failed.** Check From and To. A locator must have 4, 6 or 8 characters, and a latitude and longitude need both numbers.

**The solar data is marked stale.** Press *Refresh from NOAA*, or request the products over Winlink and import the replies.

## What the app will not do

It never keys the transmitter. There is no transmit function anywhere in it. During a scan it sets the frequency and mode, reads the radio back after every change, and puts your settings back when the scan ends.

Its predictions are monthly averages. They tell you the share of days a path should work, which is a guide to the month and not a forecast for today.

What your receiver hears depends on who is on the air. A quiet band may be open with nobody transmitting, and a station you hear may not hear you. FT8 also gets through on far less signal than SSB, so check your own mode's figure before you call.
