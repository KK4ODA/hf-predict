"""Writes radioprop-place's test streams: built from the NMEA 0183 and gpsd formats and from what
GPS2IP is documented to send, not recorded from devices."""
import json, sys
from functools import reduce
from pathlib import Path

out = Path(sys.argv[1])

def s(body):
    return "$%s*%02X" % (body, reduce(lambda x, c: x ^ ord(c), body, 0))

def ddmm(v, width):
    v = abs(v); d = int(v); m = (v - d) * 60
    return f"{d:0{width}d}{m:07.4f}"

def coords(lat, lon):
    return f"{ddmm(lat, 2)},{'N' if lat >= 0 else 'S'},{ddmm(lon, 3)},{'E' if lon >= 0 else 'W'}"

# GPS2IP (iPhone): GGA and RMC each second, HDOP fixed at 0.9 and eight satellites because iOS
# gives neither; a car going north at about 15 m/s
lines = []
for i in range(10):
    t = f"0315{22 + i:02d}.00"
    lat, lon = 33.7520 + i * 0.000135, -84.3909
    lines.append(s(f"GPGGA,{t},{coords(lat, lon)},1,08,0.9,320.5,M,0.0,M,,"))
    lines.append(s(f"GPRMC,{t},A,{coords(lat, lon)},29.2,0.0,201126,,,A"))
(out / "gps2ip.nmea").write_text("\r\n".join(lines) + "\r\n")

# a USB receiver (multi-constellation): three seconds without a fix, then fixes with GN talkers,
# satellites in view, one line with a bad checksum and one cut short
lines = []
for i in range(3):
    t = f"0315{10 + i:02d}.00"
    lines += [s(f"GPRMC,{t},V,,,,,,,201126,,,N"), s(f"GPGGA,{t},,,,,0,00,99.99,,,,,,"),
              s("GPGSA,A,1,,,,,,,,,,,,,99.99,99.99,99.99"), s("GPGSV,1,1,02,05,47,264,,13,22,054,")]
for i in range(5):
    t = f"0315{13 + i:02d}.00"
    lat, lon = 33.7490, -84.3880
    lines += [s(f"GNRMC,{t},A,{coords(lat, lon)},0.012,,201126,,,A"),
              s(f"GNGGA,{t},{coords(lat, lon)},1,09,1.12,318.2,M,-30.9,M,,"),
              s("GNGSA,A,3,05,13,15,18,20,,,,,,,,1.85,1.12,1.47"),
              s("GPGSV,2,1,07,05,47,264,32,13,22,054,28,15,61,312,35,18,30,190,31"),
              s("GLGSV,1,1,02,70,44,012,30,71,63,264,33")]
good = s(f"GNGGA,031513.00,{coords(33.7490, -84.3880)},1,09,1.12,318.2,M,-30.9,M,,")
lines.insert(16, good[:-2] + ("00" if good[-2:] != "00" else "01"))  # a wrong checksum
lines.insert(22, "$GNRMC,031514.00,A,3344.9400,N,08423.2")
(out / "usb-receiver.nmea").write_text("\r\n".join(lines) + "\r\n")

# gpsd: its greeting, the reply to WATCH, a report without a fix, then 3D fixes
reports = [
    {"class": "VERSION", "release": "3.25", "rev": "3.25", "proto_major": 3, "proto_minor": 15},
    {"class": "DEVICES", "devices": [{"class": "DEVICE", "path": "/dev/ttyACM0", "driver": "u-blox"}]},
    {"class": "WATCH", "enable": True, "json": True},
    {"class": "TPV", "device": "/dev/ttyACM0", "mode": 1, "time": "2026-11-20T03:15:20.000Z"},
    {"class": "SKY", "device": "/dev/ttyACM0", "satellites": []},
]
for i in range(3):
    reports.append({"class": "TPV", "device": "/dev/ttyACM0", "mode": 3, "status": 1,
                    "time": f"2026-11-20T03:15:{21 + i}.000Z", "lat": 33.7490, "lon": -84.3880,
                    "altHAE": 287.3, "altMSL": 318.2})
(out / "gpsd.jsonl").write_text("".join(json.dumps(r, separators=(",", ":")) + "\n" for r in reports))
