//! VOACAP as a `PropagationEngine`, run through the bundled `voacapl` binary.

pub mod deck;
pub mod output;

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::engine::Engine;
use crate::geo::LatLon;
use crate::propagation::{EngineRun, HourPrediction, PredictionRequest, PropagationEngine};

static RUN_COUNTER: AtomicU64 = AtomicU64::new(0);

pub struct VoacaplEngine {
    runner: Engine,
    run_root: PathBuf,
}

impl VoacaplEngine {
    /// `engine_root` is the folder built by `engines/voacapl/build.sh`.
    /// Each prediction gets its own folder under `run_root`.
    pub fn new(engine_root: &Path, run_root: &Path) -> Result<Self, String> {
        Ok(Self { runner: Engine::at(engine_root)?, run_root: run_root.to_path_buf() })
    }
}

impl VoacaplEngine {
    /// Runs a deck in a fresh folder and parses the output. A failed run
    /// keeps its folder so the deck and output can be inspected.
    fn run(&self, input: &str) -> Result<(crate::propagation::Prediction, String), String> {
        let run_dir = self.run_root.join(format!(
            "{}-{}",
            std::process::id(),
            RUN_COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        let kept = |e: String| format!("{e} (run folder kept at {})", run_dir.display());
        let output = self.runner.run_deck(input, &run_dir).map_err(kept)?;
        let prediction = output::parse_output(&output).map_err(kept)?;
        let _ = fs::remove_dir_all(&run_dir);
        Ok((prediction, output))
    }
}

impl PropagationEngine for VoacaplEngine {
    fn name(&self) -> &str {
        "voacapl"
    }

    /// One engine process runs every receiver: starting the process costs far
    /// more than a circuit does.
    fn predict_hour_to_many(
        &self,
        request: &PredictionRequest,
        receivers: &[LatLon],
    ) -> Result<Vec<HourPrediction>, String> {
        if request.utc_hour.is_none() {
            return Err("predicting to many receivers needs a single hour".into());
        }
        let input = deck::write_deck_for_receivers(request, receivers)?;
        let (prediction, _) = self.run(&input)?;
        if prediction.hours.len() != receivers.len() {
            return Err(format!(
                "engine returned {} results for {} receivers",
                prediction.hours.len(),
                receivers.len()
            ));
        }
        Ok(prediction.hours)
    }

    fn predict(&self, request: &PredictionRequest) -> Result<EngineRun, String> {
        let input = deck::write_deck(request)?;
        let (mut prediction, output) = self.run(&input)?;

        // The output prints frequencies to one decimal; restore the requested ones.
        for hour in &mut prediction.hours {
            if hour.frequencies.len() != request.frequencies_mhz.len() {
                return Err(format!(
                    "hour {}: engine returned {} frequencies for {} requested",
                    hour.utc_hour,
                    hour.frequencies.len(),
                    request.frequencies_mhz.len()
                ));
            }
            for (predicted, requested) in hour.frequencies.iter_mut().zip(&request.frequencies_mhz) {
                predicted.freq_mhz = *requested;
            }
        }

        Ok(EngineRun { prediction, input, output })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::predictor::{predict_overview, predict_path, PathRequest};
    use crate::propagation::FrequencyPrediction;
    use crate::solar::SsnKind;
    use crate::station::{self, Mode, HF_BANDS};
    use crate::testing::{engine_root, scratch_dir};

    fn engine(name: &str) -> VoacaplEngine {
        VoacaplEngine::new(&engine_root(), &scratch_dir(name))
            .expect("engine not built; run engines/voacapl/build.sh")
    }

    /// Each printed field with the size of one unit in its last digit.
    fn fields(f: &FrequencyPrediction) -> [(&'static str, f64, f64); 20] {
        [
            ("TANGLE", f.takeoff_angle_deg, 0.1),
            ("DELAY", f.delay_ms, 0.1),
            ("V HITE", f.virtual_height_km, 1.0),
            ("MUFday", f.muf_day, 0.01),
            ("LOSS", f.loss_db, 1.0),
            ("DBU", f.field_strength_dbu, 1.0),
            ("S DBW", f.signal_dbw, 1.0),
            ("N DBW", f.noise_dbw, 1.0),
            ("SNR", f.snr_db, 1.0),
            ("RPWRG", f.required_power_gain_db, 1.0),
            ("REL", f.reliability, 0.01),
            ("MPROB", f.multipath_probability, 0.01),
            ("S PRB", f.service_probability, 0.01),
            ("SIG LW", f.signal_lower_decile_db, 0.1),
            ("SIG UP", f.signal_upper_decile_db, 0.1),
            ("SNR LW", f.snr_lower_decile_db, 0.1),
            ("SNR UP", f.snr_upper_decile_db, 0.1),
            ("TGAIN", f.tx_gain_dbi, 0.1),
            ("RGAIN", f.rx_gain_dbi, 0.1),
            ("SNRxx", f.snr_at_required_reliability_db, 1.0),
        ]
    }

    /// The same request Windows VOACAP was given must produce the same
    /// numbers, to within one last-digit unit or 1%.
    #[test]
    fn matches_windows_voacap_reference() {
        let request = deck::tests::reference_request();
        let run = engine("reference").predict(&request).unwrap();
        let expected =
            output::parse_output(include_str!("../../../tests/engine/cases/test01.out")).unwrap();

        assert_eq!(run.prediction.hours.len(), expected.hours.len());
        for (actual, expected) in run.prediction.hours.iter().zip(&expected.hours) {
            assert_eq!(actual.utc_hour, expected.utc_hour);
            assert!((actual.muf_mhz - expected.muf_mhz).abs() <= 0.1001, "hour {} MUF", actual.utc_hour);
            assert_eq!(actual.frequencies.len(), request.frequencies_mhz.len());
            for (i, (a, e)) in actual.frequencies.iter().zip(&expected.frequencies).enumerate() {
                assert_eq!(a.freq_mhz, request.frequencies_mhz[i]);
                assert_eq!(a.mode, e.mode, "hour {} freq {} MODE", actual.utc_hour, a.freq_mhz);
                for ((label, got, unit), (_, want, _)) in fields(a).into_iter().zip(fields(e)) {
                    let allowed = (unit * 1.001).max(want.abs() * 0.01);
                    assert!(
                        (got - want).abs() <= allowed,
                        "hour {} freq {} {label}: got {got}, reference {want}",
                        actual.utc_hour,
                        a.freq_mhz
                    );
                }
            }
        }
    }

    #[test]
    fn predicts_an_amateur_path_with_station_presets() {
        let presets = station::presets();
        let request = PathRequest {
            tx_position: "EM73tr".into(),
            rx_position: "IO91wm".into(),
            year: 2026,
            month: 10,
            ssn: None,
            tx_station: presets[0].clone(),
            rx_station: presets[0].clone(),
            mode: Mode::Ft8,
            required_reliability_pct: 90.0,
            long_path: false,
        };

        let result = predict_path(&engine("amateur"), &request).unwrap();

        assert_eq!(result.engine, "voacapl");
        assert_eq!(result.ssn.kind, SsnKind::Predicted);
        assert!(result.run.prediction.engine.contains("16.1207"));
        assert!((result.run.prediction.distance_km - 6770.0).abs() < 60.0);
        assert_eq!(result.run.prediction.hours.len(), 24);
        for hour in &result.run.prediction.hours {
            assert_eq!(hour.frequencies.len(), HF_BANDS.len());
            assert!(hour.frequencies.iter().all(|f| (0.0..=1.0).contains(&f.reliability)));
        }
        // FT8 at 100 W over this path is workable on some band at some hour.
        let best = result
            .run
            .prediction
            .hours
            .iter()
            .flat_map(|h| &h.frequencies)
            .map(|f| f.reliability)
            .fold(0.0, f64::max);
        assert!(best > 0.5, "best reliability {best}");
    }

    #[test]
    fn overview_is_consistent_across_paths_powers_and_frequencies() {
        let presets = station::presets();
        let request = PathRequest {
            tx_position: "EM73tr".into(),
            rx_position: "IO91wm".into(),
            year: 2026,
            month: 10,
            ssn: None,
            tx_station: presets[0].clone(),
            rx_station: presets[0].clone(),
            mode: Mode::Ft8,
            required_reliability_pct: 90.0,
            long_path: false,
        };

        let overview = predict_overview(&engine("overview"), &request).unwrap();

        // The long way round is a different, longer circuit.
        let short = &overview.short.prediction.run.prediction;
        let long = &overview.long.prediction.run.prediction;
        assert_ne!(short.hours, long.hours);
        // The engine prints short-path geometry for both; the predictor reports the real one.
        let (short_path, long_path) = (&overview.short.prediction, &overview.long.prediction);
        assert!((short_path.distance_km - 6770.0).abs() < 60.0);
        assert!((short_path.distance_km + long_path.distance_km - 40030.0).abs() < 1.0);
        assert!((short_path.tx_bearing_deg - 45.0).abs() < 2.0);
        assert!((long_path.tx_bearing_deg - 225.0).abs() < 2.0);

        // More power never lowers reliability, and SNR rises by the power ratio in dB.
        let power = &overview.short.power;
        let (low, high) = (&power[0], &power[power.len() - 1]);
        assert_eq!((low.power_watts, high.power_watts), (5.0, 100.0));
        for hour in 0..24 {
            for band in 0..HF_BANDS.len() {
                assert!(high.reliability[hour][band] >= low.reliability[hour][band]);
                let gain = high.snr_db[hour][band] - low.snr_db[hour][band];
                assert!((gain - 13.0).abs() <= 1.0, "hour {hour} band {band}: {gain} dB");
            }
        }

        // The usable window sits below the MUF, and FT8 at 100 W finds one at some hour.
        let window = &overview.short.window;
        assert_eq!(window.len(), 24);
        for hour in window {
            if let Some(fot) = hour.fot_mhz {
                assert!(fot <= hour.muf_mhz + 0.5, "FOT {fot} above MUF {}", hour.muf_mhz);
            }
        }
        assert!(window.iter().any(|h| matches!((h.luf_mhz, h.fot_mhz), (Some(l), Some(f)) if l < f)));
    }

    #[test]
    fn concurrent_predictions_do_not_collide() {
        let engine = engine("concurrent");
        let request = deck::tests::reference_request();
        let first = engine.predict(&request).unwrap().prediction;
        std::thread::scope(|scope| {
            let runs: Vec<_> = (0..4).map(|_| scope.spawn(|| engine.predict(&request))).collect();
            for run in runs {
                assert_eq!(run.join().unwrap().unwrap().prediction, first);
            }
        });
    }
}
