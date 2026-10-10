//! A client for Hamlib's `rigctld`, using its extended response protocol
//! (commands prefixed with `+`), so that every reply ends in an `RPRT` line
//! and can be framed and checked. This client only ever sends `get`
//! commands.

use std::io::{BufRead, BufReader, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

use super::{RadioController, RadioState};
use crate::station;

const TIMEOUT: Duration = Duration::from_secs(3);

pub struct Rigctld {
    reader: BufReader<TcpStream>,
    name: String,
}

impl Rigctld {
    pub fn connect(host: &str, port: u16) -> Result<Self, String> {
        let target = format!("{host}:{port}");
        let address = (host, port)
            .to_socket_addrs()
            .map_err(|e| format!("{target}: {e}"))?
            .next()
            .ok_or_else(|| format!("{target}: no address"))?;
        let stream = TcpStream::connect_timeout(&address, TIMEOUT)
            .map_err(|e| format!("cannot connect to rigctld at {target}: {e}"))?;
        stream.set_read_timeout(Some(TIMEOUT)).map_err(|e| e.to_string())?;
        stream.set_write_timeout(Some(TIMEOUT)).map_err(|e| e.to_string())?;
        let _ = stream.set_nodelay(true);
        Ok(Self { reader: BufReader::new(stream), name: format!("rigctld at {target}") })
    }

    /// Sends one read command, `get_freq` and the like, and returns the
    /// fields of its reply.
    pub fn query(&mut self, command: &str) -> Result<Vec<(String, String)>, String> {
        debug_assert!(command.starts_with("get_") || command == "dump_state");
        self.reader
            .get_mut()
            .write_all(format!("+\\{command}\n").as_bytes())
            .map_err(|e| format!("rigctld: cannot send {command}: {e}"))?;
        let mut lines = Vec::new();
        loop {
            let mut line = String::new();
            let read = self.reader.read_line(&mut line).map_err(|e| format!("rigctld: no reply to {command}: {e}"))?;
            if read == 0 {
                return Err("rigctld closed the connection".into());
            }
            lines.push(line.trim_end().to_string());
            if line.starts_with("RPRT ") {
                return parse_reply(command, &lines);
            }
        }
    }
}

/// Hamlib's error codes, as `rigctld` reports them.
pub fn describe(code: i32) -> String {
    let text = match code {
        -1 => "invalid parameter",
        -2 => "invalid configuration",
        -3 => "memory shortage",
        -4 => "feature not available",
        -5 => "communication with the radio timed out",
        -6 => "I/O error talking to the radio",
        -7 => "internal Hamlib error",
        -8 => "protocol error",
        -9 => "command rejected by the radio",
        -10 => "argument truncated",
        -11 => "function not available",
        -12 => "VFO not targetable",
        -13 => "bus error",
        -14 => "bus busy",
        -15 => "invalid argument",
        -16 => "invalid VFO",
        -17 => "argument out of range",
        _ => return format!("rigctld error {code}"),
    };
    format!("{text} (RPRT {code})")
}

/// The lines of one extended-protocol reply: the command echo, `Key: value`
/// fields, then `RPRT n`.
pub fn parse_reply(command: &str, lines: &[String]) -> Result<Vec<(String, String)>, String> {
    let report = lines.last().and_then(|l| l.strip_prefix("RPRT ")).ok_or("rigctld: reply without a report line")?;
    let code: i32 = report.trim().parse().map_err(|_| format!("rigctld: bad report line {report:?}"))?;
    if code != 0 {
        return Err(format!("{command}: {}", describe(code)));
    }
    Ok(lines[..lines.len() - 1]
        .iter()
        .filter_map(|line| line.split_once(':'))
        .filter(|(key, _)| *key != command)
        .map(|(key, value)| (key.trim().to_string(), value.trim().to_string()))
        .collect())
}

fn field<'a>(fields: &'a [(String, String)], key: &str) -> Option<&'a str> {
    fields.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
}

impl RadioController for Rigctld {
    fn name(&self) -> &str {
        &self.name
    }

    fn read(&mut self) -> Result<RadioState, String> {
        let freq = self.query("get_freq")?;
        let freq_hz: u64 = field(&freq, "Frequency")
            .ok_or("rigctld: no Frequency in the reply")?
            .parse::<f64>()
            .map_err(|e| format!("rigctld: bad frequency: {e}"))? as u64;
        let mode = self.query("get_mode")?;
        let ptt = self.query("get_ptt")?;
        // Split and VFO are not supported by every radio; their absence is not a failure.
        let split = self.query("get_split_vfo").ok();
        let vfo = self.query("get_vfo").ok();
        Ok(RadioState {
            freq_hz,
            band: station::band_for_hz(freq_hz),
            mode: field(&mode, "Mode").unwrap_or("?").to_string(),
            passband_hz: field(&mode, "Passband").and_then(|p| p.parse().ok()),
            ptt: field(&ptt, "PTT").is_some_and(|p| p != "0"),
            split: split.as_deref().and_then(|s| field(s, "Split")).map(|s| s != "0"),
            vfo: vfo.as_deref().and_then(|v| field(v, "VFO")).map(String::from),
            tx_vfo: split.as_deref().and_then(|s| field(s, "TX VFO")).map(String::from),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(text: &str) -> Vec<String> {
        text.lines().map(String::from).collect()
    }

    #[test]
    fn reads_fields_and_checks_the_report() {
        let reply = parse_reply("get_mode", &lines("get_mode:\nMode: USB\nPassband: 3000\nRPRT 0")).unwrap();
        assert_eq!(reply, [("Mode".to_string(), "USB".to_string()), ("Passband".to_string(), "3000".to_string())]);
        assert_eq!(
            parse_reply("get_split_vfo", &lines("get_split_vfo:\nSplit: 1\nTX VFO: VFOB\nRPRT 0")).unwrap(),
            [("Split".to_string(), "1".to_string()), ("TX VFO".to_string(), "VFOB".to_string())]
        );
    }

    #[test]
    fn errors_are_named() {
        let error = parse_reply("get_freq", &lines("get_freq:\nRPRT -5")).unwrap_err();
        assert_eq!(error, "get_freq: communication with the radio timed out (RPRT -5)");
        assert_eq!(describe(-42), "rigctld error -42");
        assert!(parse_reply("get_freq", &lines("get_freq:\nFrequency: 1")).unwrap_err().contains("without a report"));
    }
}
