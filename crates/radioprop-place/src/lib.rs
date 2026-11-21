// SPDX-License-Identifier: MIT OR Apache-2.0
//! Where an amateur station is: saved places, a GPS, and which of them gives the position in use.
//!
//! Shared by hf-predict and RadioPost (RadioPost's design, *Choosing a gateway*, §6 and §11), so
//! that both read a GPS the same way. Each program keeps its own [`Book`] in a file of its own.
//!
//! * [`nmea`] reads NMEA 0183 sentences: a position only from `RMC` status `A` or `GGA` quality 1
//!   or more, any talker, never satellite counts or HDOP as quality.
//! * [`gpsd`] reads gpsd's JSON reports.
//! * [`track`] keeps what a GPS has said: freshness (current, recent, stale), a jump held until a
//!   second fix confirms it, a GPS silent or without a fix. No input or output: the clock is
//!   handed in.
//! * [`read`] runs a reader on a thread: a serial port (its speed given or found), a TCP
//!   connection, a UDP port, or gpsd.
//! * [`place`] keeps the places and chooses the position in use.
//! * [`station`] holds station profiles: power, antennas, feed line and noise, every value set or
//!   assumed.

pub mod gpsd;
pub mod nmea;
pub mod place;
pub mod read;
pub mod station;
pub mod track;

pub use nmea::Quality;
pub use place::{choose, Book, Choice, Chosen, Here, Origin, Place};
pub use read::{now_ms, serial_ports, Gps, Link, Source, Status};
pub use station::{Resolved, StationProfile};
pub use track::{Fix, Freshness, Hearing, Tracker};
