# Test streams

What the tests feed the parsers and the readers.

**They are built, not recorded.** `../../tools/make_streams.py` writes them from the NMEA 0183 and
gpsd formats and from what GPS2IP is documented to send (GGA and RMC each second, with a fixed HDOP
of 0.9 and eight satellites, because iOS gives neither). Recordings from real devices should replace
them: a USB receiver, a Bluetooth receiver, GPS2IP in its TCP and UDP modes, Share GPS, and gpsd.
Record a minute from each, with the time it takes the receiver to find its fix, and keep the file
names.

| File | Stands for |
|---|---|
| `gps2ip.nmea` | An iPhone running GPS2IP in a car going north at about 15 m/s |
| `usb-receiver.nmea` | A multi-constellation USB receiver: three seconds without a fix, then fixes with `GN` talkers and satellites in view; one sentence with a wrong checksum and one cut short |
| `gpsd.jsonl` | gpsd's greeting, its answer to `?WATCH`, a report without a fix, then three 3D fixes |
