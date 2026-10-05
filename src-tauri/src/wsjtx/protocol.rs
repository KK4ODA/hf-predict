//! The WSJT-X UDP message format, as documented in WSJT-X's
//! `Network/NetworkMessage.hpp`: big-endian Qt data-stream fields after a
//! magic number, schema number, message type and sender id.
//!
//! WSJT-X adds message types and trailing fields without changing the schema
//! number, so unknown types are reported as `Other` and extra bytes ignored.

pub const MAGIC: u32 = 0xadbc_cbda;

const HEARTBEAT: u32 = 0;
const STATUS: u32 = 1;
const DECODE: u32 = 2;
const CLEAR: u32 = 3;
const CLOSE: u32 = 6;

#[derive(Debug, Clone, PartialEq)]
pub struct Status {
    pub dial_hz: u64,
    pub mode: String,
    pub tx_enabled: bool,
    pub transmitting: bool,
    pub decoding: bool,
    /// Fields later versions append; absent from older senders.
    pub de_call: Option<String>,
    pub de_grid: Option<String>,
    /// Transmit/receive period in seconds.
    pub tr_period_s: Option<u32>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Decode {
    /// False for decodes re-sent on request rather than fresh off the air.
    pub new: bool,
    /// Milliseconds since UTC midnight; the message carries no date.
    pub time_ms: u32,
    pub snr_db: i32,
    /// Time offset of the signal against the receiver's clock, in seconds.
    pub dt_s: f64,
    /// Audio offset in Hz above the dial frequency.
    pub df_hz: u32,
    /// Mode symbol as WSJT-X prints it (`~` for FT8, `+` for FT4).
    pub mode: String,
    pub message: String,
    pub low_confidence: bool,
    /// True for decodes of a played-back recording.
    pub off_air: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Body {
    Heartbeat { version: Option<String> },
    Status(Status),
    Decode(Decode),
    Clear,
    Close,
    /// A message type this app does not use.
    Other(u32),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Message {
    /// The sending program's instance name, e.g. `WSJT-X`.
    pub id: String,
    pub body: Body,
}

struct Reader<'a> {
    data: &'a [u8],
}

impl<'a> Reader<'a> {
    fn take(&mut self, count: usize) -> Result<&'a [u8], String> {
        if self.data.len() < count {
            return Err("datagram ends early".into());
        }
        let (head, rest) = self.data.split_at(count);
        self.data = rest;
        Ok(head)
    }

    fn u8(&mut self) -> Result<u8, String> {
        Ok(self.take(1)?[0])
    }

    fn bool(&mut self) -> Result<bool, String> {
        Ok(self.u8()? != 0)
    }

    fn u32(&mut self) -> Result<u32, String> {
        Ok(u32::from_be_bytes(self.take(4)?.try_into().unwrap()))
    }

    fn i32(&mut self) -> Result<i32, String> {
        Ok(self.u32()? as i32)
    }

    fn u64(&mut self) -> Result<u64, String> {
        Ok(u64::from_be_bytes(self.take(8)?.try_into().unwrap()))
    }

    fn f64(&mut self) -> Result<f64, String> {
        Ok(f64::from_be_bytes(self.take(8)?.try_into().unwrap()))
    }

    /// A length-prefixed UTF-8 string; length `0xffffffff` is Qt's null string.
    fn string(&mut self) -> Result<String, String> {
        match self.u32()? {
            u32::MAX => Ok(String::new()),
            length => Ok(String::from_utf8_lossy(self.take(length as usize)?).into_owned()),
        }
    }
}

pub fn parse(datagram: &[u8]) -> Result<Message, String> {
    let mut r = Reader { data: datagram };
    if r.u32()? != MAGIC {
        return Err("not a WSJT-X datagram".into());
    }
    let _schema = r.u32()?;
    let kind = r.u32()?;
    let id = r.string()?;

    let body = match kind {
        HEARTBEAT => {
            // Old senders stop after the id.
            let _max_schema = r.u32().ok();
            Body::Heartbeat { version: r.string().ok() }
        }
        STATUS => {
            let dial_hz = r.u64()?;
            let mode = r.string()?;
            let _dx_call = r.string()?;
            let _report = r.string()?;
            let _tx_mode = r.string()?;
            let tx_enabled = r.bool()?;
            let transmitting = r.bool()?;
            let decoding = r.bool()?;
            // Everything after this was added over time; read what is there.
            let mut later = || -> Result<(String, String, Option<u32>), String> {
                let _rx_df = r.u32()?;
                let _tx_df = r.u32()?;
                let de_call = r.string()?;
                let de_grid = r.string()?;
                let period = (|| {
                    let _dx_grid = r.string()?;
                    let _tx_watchdog = r.bool()?;
                    let _sub_mode = r.string()?;
                    let _fast_mode = r.bool()?;
                    let _special_operation = r.u8()?;
                    let _frequency_tolerance = r.u32()?;
                    r.u32()
                })()
                .ok()
                // Qt's "no value" for this field is the largest number.
                .filter(|period| *period != u32::MAX);
                Ok((de_call, de_grid, period))
            };
            let (de_call, de_grid, tr_period_s) = match later() {
                Ok((call, grid, period)) => (Some(call), Some(grid), period),
                Err(_) => (None, None, None),
            };
            Body::Status(Status {
                dial_hz,
                mode,
                tx_enabled,
                transmitting,
                decoding,
                de_call,
                de_grid,
                tr_period_s,
            })
        }
        DECODE => Body::Decode(Decode {
            new: r.bool()?,
            time_ms: r.u32()?,
            snr_db: r.i32()?,
            dt_s: r.f64()?,
            df_hz: r.u32()?,
            mode: r.string()?,
            message: r.string()?,
            low_confidence: r.bool().unwrap_or(false),
            off_air: r.bool().unwrap_or(false),
        }),
        CLEAR => Body::Clear,
        CLOSE => Body::Close,
        other => Body::Other(other),
    };
    Ok(Message { id, body })
}

/// Builds datagrams in the same format, for tests and replayed sessions.
pub mod encode {
    use super::{Decode, Status, CLOSE, DECODE, HEARTBEAT, MAGIC, STATUS};

    const SCHEMA: u32 = 2;

    struct Writer(Vec<u8>);

    impl Writer {
        fn new(kind: u32, id: &str) -> Self {
            let mut w = Writer(Vec::new());
            w.u32(MAGIC).u32(SCHEMA).u32(kind).string(id);
            w
        }
        fn u8(&mut self, value: u8) -> &mut Self {
            self.0.push(value);
            self
        }
        fn u32(&mut self, value: u32) -> &mut Self {
            self.0.extend_from_slice(&value.to_be_bytes());
            self
        }
        fn u64(&mut self, value: u64) -> &mut Self {
            self.0.extend_from_slice(&value.to_be_bytes());
            self
        }
        fn f64(&mut self, value: f64) -> &mut Self {
            self.0.extend_from_slice(&value.to_be_bytes());
            self
        }
        fn string(&mut self, value: &str) -> &mut Self {
            self.u32(value.len() as u32);
            self.0.extend_from_slice(value.as_bytes());
            self
        }
    }

    pub fn heartbeat(id: &str, version: &str) -> Vec<u8> {
        let mut w = Writer::new(HEARTBEAT, id);
        w.u32(3).string(version).string("");
        w.0
    }

    pub fn status(id: &str, status: &Status) -> Vec<u8> {
        let mut w = Writer::new(STATUS, id);
        w.u64(status.dial_hz)
            .string(&status.mode)
            .string("")
            .string("")
            .string(&status.mode)
            .u8(status.tx_enabled as u8)
            .u8(status.transmitting as u8)
            .u8(status.decoding as u8);
        if let (Some(call), Some(grid)) = (&status.de_call, &status.de_grid) {
            w.u32(1500).u32(1500).string(call).string(grid);
            if let Some(period) = status.tr_period_s {
                w.string("").u8(0).string("").u8(0).u8(0).u32(u32::MAX).u32(period);
            }
        }
        w.0
    }

    pub fn decode(id: &str, decode: &Decode) -> Vec<u8> {
        let mut w = Writer::new(DECODE, id);
        w.u8(decode.new as u8)
            .u32(decode.time_ms)
            .u32(decode.snr_db as u32)
            .f64(decode.dt_s)
            .u32(decode.df_hz)
            .string(&decode.mode)
            .string(&decode.message)
            .u8(decode.low_confidence as u8)
            .u8(decode.off_air as u8);
        w.0
    }

    pub fn close(id: &str) -> Vec<u8> {
        Writer::new(CLOSE, id).0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn status() -> Status {
        Status {
            dial_hz: 14_074_000,
            mode: "FT8".into(),
            tx_enabled: false,
            transmitting: false,
            decoding: true,
            de_call: Some("N0CALL".into()),
            de_grid: Some("EM73".into()),
            tr_period_s: Some(15),
        }
    }

    fn decode() -> Decode {
        Decode {
            new: true,
            time_ms: 55_815_000,
            snr_db: -12,
            dt_s: 0.3,
            df_hz: 1234,
            mode: "~".into(),
            message: "CQ K1ABC FN42".into(),
            low_confidence: false,
            off_air: false,
        }
    }

    /// A heartbeat written out byte by byte from the format description, so
    /// the parser is checked against the specification and not only against
    /// this module's own encoder.
    #[test]
    fn reads_a_hand_built_heartbeat() {
        let datagram = [
            0xad, 0xbc, 0xcb, 0xda, // magic
            0, 0, 0, 2, // schema
            0, 0, 0, 0, // type: heartbeat
            0, 0, 0, 6, b'W', b'S', b'J', b'T', b'-', b'X', // id
            0, 0, 0, 3, // maximum schema
            0, 0, 0, 5, b'3', b'.', b'0', b'.', b'2', // version
            0xff, 0xff, 0xff, 0xff, // revision: null string
        ];
        assert_eq!(
            parse(&datagram).unwrap(),
            Message { id: "WSJT-X".into(), body: Body::Heartbeat { version: Some("3.0.2".into()) } }
        );
    }

    #[test]
    fn reads_status_and_decode() {
        let parsed = parse(&encode::status("WSJT-X", &status())).unwrap();
        assert_eq!(parsed, Message { id: "WSJT-X".into(), body: Body::Status(status()) });

        let parsed = parse(&encode::decode("WSJT-X", &decode())).unwrap();
        assert_eq!(parsed.body, Body::Decode(decode()));
        assert_eq!(parse(&encode::close("WSJT-X")).unwrap().body, Body::Close);
    }

    #[test]
    fn negative_snr_survives() {
        let mut d = decode();
        d.snr_db = -24;
        let Body::Decode(parsed) = parse(&encode::decode("x", &d)).unwrap().body else { panic!() };
        assert_eq!(parsed.snr_db, -24);
    }

    #[test]
    fn older_senders_with_shorter_messages_are_read() {
        // A status that stops after the decoding flag.
        let mut short = status();
        short.de_call = None;
        short.de_grid = None;
        short.tr_period_s = None;
        let Body::Status(parsed) = parse(&encode::status("JTDX", &short)).unwrap().body else { panic!() };
        assert_eq!(parsed, short);

        // A status with callsign and grid but nothing after.
        let mut medium = status();
        medium.tr_period_s = None;
        let Body::Status(parsed) = parse(&encode::status("JTDX", &medium)).unwrap().body else { panic!() };
        assert_eq!(parsed, medium);

        // A decode without the two trailing flags.
        let datagram = encode::decode("x", &decode());
        let Body::Decode(parsed) = parse(&datagram[..datagram.len() - 2]).unwrap().body else { panic!() };
        assert_eq!(parsed, decode());
    }

    #[test]
    fn newer_senders_with_extra_bytes_or_types_are_tolerated() {
        let mut datagram = encode::decode("x", &decode());
        datagram.extend_from_slice(&[1, 2, 3, 4]);
        assert_eq!(parse(&datagram).unwrap().body, Body::Decode(decode()));

        // Type 17 (InhibitStatus) arrived in WSJT-X 3.2.
        let mut unknown = encode::close("x");
        unknown[11] = 17;
        assert_eq!(parse(&unknown).unwrap().body, Body::Other(17));
    }

    #[test]
    fn rejects_foreign_and_truncated_datagrams() {
        assert!(parse(b"hello world, this is not wsjt-x").unwrap_err().contains("not a WSJT-X"));
        assert!(parse(&[]).is_err());
        let datagram = encode::decode("x", &decode());
        assert!(parse(&datagram[..20]).unwrap_err().contains("ends early"));
    }
}
