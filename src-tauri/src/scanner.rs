//! Moves the radio through a listening plan: the app's one writer to the
//! rig. It retunes only when every rule holds, reads back after each
//! retune, pauses while WSJT-X is working a station, and puts the radio
//! back where it was when it stops for any reason. The decisions are a
//! pure function of time and what the radio and WSJT-X report, so they are
//! tested without a radio; a runner thread carries them out.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::JoinHandle;
use std::time::Duration;

use serde::Serialize;

use crate::radio::{Monitor, RadioState};
use crate::scan::{Plan, PlanItem};
use crate::timeutil;

/// WSJT-X is taken to have gone away after this long without a message.
pub const REPORTING_WITHIN_S: i64 = 30;
const TICK: Duration = Duration::from_millis(250);

/// What WSJT-X reports, as far as the scanner cares.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct WsjtxState {
    /// A Status message arrived recently.
    pub reporting: bool,
    pub tx_enabled: bool,
    pub transmitting: bool,
}

/// Why a scan may not begin. Empty means it may.
pub fn preflight(radio: Option<&RadioState>, wsjtx: WsjtxState, confirmed: bool) -> Vec<String> {
    let mut reasons = Vec::new();
    match radio {
        None => reasons.push("the radio is not connected".to_string()),
        Some(radio) => {
            if radio.ptt {
                reasons.push("the radio is transmitting".to_string());
            }
            if radio.split == Some(true) {
                reasons.push("split is on at the radio; set WSJT-X's split to None or Fake It".to_string());
            }
        }
    }
    if !wsjtx.reporting {
        reasons.push("WSJT-X is not reporting over UDP".to_string());
    }
    if wsjtx.tx_enabled {
        reasons.push("WSJT-X has transmit enabled; disable it to scan".to_string());
    }
    if !confirmed {
        reasons.push("confirm that the antenna system is safe to retune on receive".to_string());
    }
    reasons
}

/// The frequency and mode after a retune must be what was asked for and
/// the mode the radio had: a band change can recall another stored mode on
/// some radios.
pub fn verify_retune(expected_hz: u64, saved: &RadioState, after: &RadioState) -> Result<(), String> {
    if after.freq_hz != expected_hz {
        return Err(format!("the radio reads {} Hz after being set to {expected_hz} Hz", after.freq_hz));
    }
    if after.mode != saved.mode {
        return Err(format!("the mode changed from {} to {} on retuning", saved.mode, after.mode));
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    /// Set the radio to this frequency.
    Retune(u64),
    /// The plan has run its course, or a rule stopped it.
    Finish(String),
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Step {
    pub action: Option<Action>,
    /// Why nothing is being done right now.
    pub paused: Option<String>,
    /// The plan item in force.
    pub item: Option<usize>,
}

/// The decisions, without the radio.
pub struct Scanner {
    plan: Plan,
    started_s: i64,
    item: Option<usize>,
    pub retunes: u32,
}

impl Scanner {
    pub fn new(plan: Plan, started_s: i64) -> Self {
        Self { plan, started_s, item: None, retunes: 0 }
    }

    pub fn plan(&self) -> &Plan {
        &self.plan
    }

    /// Seconds the plan runs for: to the end of its last item.
    pub fn length_s(&self) -> i64 {
        self.plan.items.last().map_or(0, |i| i64::from(i.start_s + i.dwell_s))
    }

    pub fn step(&mut self, now_s: i64, radio: &RadioState, wsjtx: WsjtxState) -> Step {
        let mut step = Step { item: self.item, ..Step::default() };
        if radio.split == Some(true) {
            step.action = Some(Action::Finish("split came on at the radio".into()));
            return step;
        }
        let paused = if radio.ptt {
            Some("the radio is transmitting")
        } else if wsjtx.transmitting {
            Some("WSJT-X is transmitting")
        } else if wsjtx.tx_enabled {
            Some("WSJT-X has transmit enabled: the operator is working a station")
        } else if !wsjtx.reporting {
            Some("WSJT-X stopped reporting")
        } else {
            None
        };
        if let Some(reason) = paused {
            step.paused = Some(reason.into());
            return step;
        }
        let elapsed = now_s - self.started_s;
        if elapsed >= self.length_s() {
            step.action = Some(Action::Finish("the plan ran its course".into()));
            return step;
        }
        let due = self.plan.items.iter().rposition(|i| i64::from(i.start_s) <= elapsed);
        if due != self.item {
            if let Some(index) = due {
                self.item = Some(index);
                self.retunes += 1;
                step.item = Some(index);
                step.action = Some(Action::Retune(self.plan.items[index].dial_hz));
            }
        }
        step
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanStatus {
    /// `idle`, `running`, `paused`, `stopped` or `failed`.
    pub state: &'static str,
    pub detail: String,
    pub started_utc: Option<i64>,
    pub current: Option<PlanItem>,
    pub next: Option<PlanItem>,
    pub retunes: u32,
    /// Where the radio was when the scan began, and goes back to.
    pub saved: Option<RadioState>,
    pub restored: bool,
    pub plans_run: u32,
}

impl ScanStatus {
    pub fn idle() -> Self {
        Self {
            state: "idle",
            detail: "Not scanning.".into(),
            started_utc: None,
            current: None,
            next: None,
            retunes: 0,
            saved: None,
            restored: false,
            plans_run: 0,
        }
    }
}

type WsjtxSource = Box<dyn Fn() -> WsjtxState + Send>;
type Replan = Box<dyn Fn() -> Result<Plan, String> + Send>;

/// Carries a scan out on a thread. Dropping it stops the scan and waits
/// for the radio to be restored.
pub struct Runner {
    stop: Arc<AtomicBool>,
    status: Arc<Mutex<ScanStatus>>,
    thread: Option<JoinHandle<()>>,
}

impl Runner {
    /// Refuses to start unless every rule holds. `replan` makes the next
    /// plan when one runs its course; without it the scan stops then.
    pub fn start(
        plan: Plan,
        monitor: Arc<Monitor>,
        wsjtx: WsjtxSource,
        confirmed: bool,
        replan: Option<Replan>,
    ) -> Result<Self, String> {
        let radio = monitor.status().radio;
        let reasons = preflight(radio.as_ref(), wsjtx(), confirmed);
        if !reasons.is_empty() {
            return Err(format!("Cannot scan: {}.", reasons.join("; ")));
        }
        if plan.items.is_empty() {
            return Err("Cannot scan: the plan is empty.".into());
        }
        let saved = radio.ok_or("the radio is not connected")?;
        let stop = Arc::new(AtomicBool::new(false));
        let status = Arc::new(Mutex::new(ScanStatus {
            state: "running",
            detail: "Scanning.".into(),
            started_utc: Some(timeutil::now()),
            saved: Some(saved.clone()),
            ..ScanStatus::idle()
        }));
        let thread = {
            let (stop, status) = (stop.clone(), status.clone());
            std::thread::spawn(move || run(plan, saved, monitor, wsjtx, replan, stop, status))
        };
        Ok(Self { stop, status, thread: Some(thread) })
    }

    pub fn status(&self) -> ScanStatus {
        self.status.lock().unwrap_or_else(PoisonError::into_inner).clone()
    }

    /// Asks the scan to stop; the radio is restored on the runner's thread.
    pub fn stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
    }

    pub fn finished(&self) -> bool {
        matches!(self.status().state, "stopped" | "failed")
    }
}

impl Drop for Runner {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn run(
    plan: Plan,
    saved: RadioState,
    monitor: Arc<Monitor>,
    wsjtx: WsjtxSource,
    replan: Option<Replan>,
    stop: Arc<AtomicBool>,
    status: Arc<Mutex<ScanStatus>>,
) {
    let update = |f: &dyn Fn(&mut ScanStatus)| f(&mut status.lock().unwrap_or_else(PoisonError::into_inner));
    let mut scanner = Scanner::new(plan, timeutil::now());
    let outcome: Result<String, String> = loop {
        if stop.load(Ordering::Relaxed) {
            break Ok("stopped by the operator".into());
        }
        let radio_status = monitor.status();
        let Some(radio) = radio_status.radio.filter(|_| radio_status.state == "connected") else {
            break Err("the radio connection was lost".into());
        };
        let step = scanner.step(timeutil::now(), &radio, wsjtx());
        let items = &scanner.plan().items;
        let current = step.item.map(|i| items[i].clone());
        let next = step.item.and_then(|i| items.get(i + 1).cloned());
        let retunes = scanner.retunes;
        update(&|s| {
            s.current = current.clone();
            s.next = next.clone();
            s.retunes = retunes;
            match &step.paused {
                Some(reason) => {
                    s.state = "paused";
                    s.detail = format!("Paused: {reason}.");
                }
                None => {
                    s.state = "running";
                    s.detail = "Scanning.".into();
                }
            }
        });
        match step.action {
            Some(Action::Retune(hz)) => match monitor.set_freq(hz).and_then(|after| verify_retune(hz, &saved, &after)) {
                Ok(()) => {}
                Err(e) => break Err(e),
            },
            Some(Action::Finish(reason)) => match &replan {
                Some(make) if !stop.load(Ordering::Relaxed) => match make() {
                    Ok(plan) if !plan.items.is_empty() => {
                        scanner = Scanner::new(plan, timeutil::now());
                        update(&|s| s.plans_run += 1);
                    }
                    Ok(_) => break Ok("the new plan was empty".into()),
                    Err(e) => break Err(format!("could not make the next plan: {e}")),
                },
                _ => break Ok(reason),
            },
            None => {}
        }
        std::thread::sleep(TICK);
    };

    // Whatever happened, put the radio back.
    let restored = monitor
        .set_freq(saved.freq_hz)
        .and_then(|after| verify_retune(saved.freq_hz, &saved, &after));
    update(&|s| {
        s.current = None;
        s.next = None;
        s.restored = restored.is_ok();
        let back = match &restored {
            Ok(()) => format!("The radio is back on {:.3} MHz.", saved.freq_hz as f64 / 1e6),
            Err(e) => format!(
                "The radio could not be put back ({e}); set it to {:.3} MHz by hand.",
                saved.freq_hz as f64 / 1e6
            ),
        };
        match &outcome {
            Ok(reason) => {
                s.state = "stopped";
                s.detail = format!("Scan ended: {reason}. {back}");
            }
            Err(e) => {
                s.state = "failed";
                s.detail = format!("Scan stopped: {e}. {back}");
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compare::ObservedTier;
    use crate::scan::{self, BandInput};

    fn plan() -> Plan {
        let inputs: Vec<BandInput> = [("20 m", 14_074_000u64, 0.9), ("40 m", 7_074_000, 0.8), ("15 m", 21_074_000, 0.7)]
            .iter()
            .map(|(band, hz, p)| BandInput {
                band: band.to_string(),
                dial_hz: *hz,
                prediction: *p,
                observed: ObservedTier::NotSampled,
                minutes_since_listened: Some(10.0),
                excluded: false,
            })
            .collect();
        scan::plan(&inputs, 10)
    }

    fn ok() -> WsjtxState {
        WsjtxState { reporting: true, tx_enabled: false, transmitting: false }
    }

    #[test]
    fn preflight_names_every_rule_that_fails() {
        let mut radio = RadioState::at(14_074_000, "USB");
        assert!(preflight(Some(&radio), ok(), true).is_empty());
        radio.ptt = true;
        radio.split = Some(true);
        let reasons = preflight(Some(&radio), WsjtxState { tx_enabled: true, ..WsjtxState::default() }, false);
        assert_eq!(reasons.len(), 5, "{reasons:?}");
        assert_eq!(preflight(None, ok(), true), ["the radio is not connected"]);
        assert_eq!(
            preflight(Some(&RadioState::at(1, "USB")), WsjtxState { tx_enabled: true, ..ok() }, true),
            ["WSJT-X has transmit enabled; disable it to scan"]
        );
    }

    #[test]
    fn retunes_at_each_item_in_order_and_finishes_at_the_end() {
        let plan = plan();
        let starts: Vec<u32> = plan.items.iter().map(|i| i.start_s).collect();
        let length = plan.items.last().map(|i| i.start_s + i.dwell_s).unwrap();
        let mut scanner = Scanner::new(plan.clone(), 1000);
        let radio = RadioState::at(14_074_000, "USB");

        let first = scanner.step(1000, &radio, ok());
        assert_eq!(first.action, Some(Action::Retune(plan.items[0].dial_hz)));
        assert_eq!(first.item, Some(0));
        // Nothing to do until the next item is due.
        assert_eq!(scanner.step(1000 + i64::from(starts[1]) - 1, &radio, ok()).action, None);
        let second = scanner.step(1000 + i64::from(starts[1]), &radio, ok());
        assert_eq!(second.action, Some(Action::Retune(plan.items[1].dial_hz)));
        assert_eq!(scanner.retunes, 2);
        let end = scanner.step(1000 + i64::from(length), &radio, ok());
        assert_eq!(end.action, Some(Action::Finish("the plan ran its course".into())));
    }

    #[test]
    fn pauses_while_the_operator_is_working_and_stops_if_split_comes_on() {
        let plan = plan();
        let mut scanner = Scanner::new(plan.clone(), 0);
        let mut radio = RadioState::at(14_074_000, "USB");
        let busy = WsjtxState { tx_enabled: true, ..ok() };
        let paused = scanner.step(0, &radio, busy);
        assert_eq!((paused.action, paused.paused.is_some()), (None, true));
        assert_eq!(scanner.retunes, 0);
        // Transmit gets disabled a while later: the item due now is taken up.
        let later = i64::from(plan.items[1].start_s) + 5;
        let resumed = scanner.step(later, &radio, ok());
        assert_eq!(resumed.action, Some(Action::Retune(plan.items[1].dial_hz)));
        let ptt = scanner.step(later + 1, &RadioState { ptt: true, ..radio.clone() }, ok());
        assert_eq!(ptt.paused.as_deref(), Some("the radio is transmitting"));
        assert_eq!(scanner.step(later + 2, &radio, WsjtxState { reporting: false, ..ok() }).paused.as_deref(), Some("WSJT-X stopped reporting"));
        radio.split = Some(true);
        assert_eq!(scanner.step(later + 3, &radio, ok()).action, Some(Action::Finish("split came on at the radio".into())));
    }

    #[test]
    fn a_retune_must_land_where_asked_with_the_mode_unchanged() {
        let saved = RadioState::at(14_074_000, "USB");
        assert!(verify_retune(7_074_000, &saved, &RadioState::at(7_074_000, "USB")).is_ok());
        assert!(verify_retune(7_074_000, &saved, &RadioState::at(7_074_100, "USB")).unwrap_err().contains("7074100"));
        assert!(verify_retune(7_074_000, &saved, &RadioState::at(7_074_000, "CW")).unwrap_err().contains("USB to CW"));
    }
}
