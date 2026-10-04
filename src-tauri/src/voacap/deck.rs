//! Writes VOACAP input decks. Cards are fixed-column; the layouts follow the
//! engine's own `read` formats.

use crate::propagation::{Antenna, Coefficients, PathKind, PredictionRequest};

const MAX_FREQUENCIES: usize = 11;
const ANTENNA_FILE_WIDTH: usize = 21;
const LABEL_WIDTH: usize = 20;

pub fn write_deck(request: &PredictionRequest) -> Result<String, String> {
    let frequencies = &request.frequencies_mhz;
    if frequencies.is_empty() || frequencies.len() > MAX_FREQUENCIES {
        return Err(format!("a deck takes 1 to {MAX_FREQUENCIES} frequencies, got {}", frequencies.len()));
    }
    if let Some(bad) = frequencies.iter().find(|f| !(2.0..=30.0).contains(*f)) {
        return Err(format!("{bad} MHz is outside the model's 2-30 MHz range"));
    }
    if !(1..=12).contains(&request.month) {
        return Err(format!("month {} is not 1-12", request.month));
    }
    if request.tx_power_watts <= 0.0 {
        return Err("transmit power must be positive".into());
    }

    let coefficients = match request.coefficients {
        Coefficients::Ccir => "CCIR",
        Coefficients::Ursi => "URSI",
    };
    let path = match request.path {
        PathKind::Short => 'S',
        PathKind::Long => 'L',
    };
    let (tx_lat, tx_ns) = hemisphere(request.tx.lat, 'N', 'S');
    let (tx_lon, tx_ew) = hemisphere(request.tx.lon, 'E', 'W');
    let (rx_lat, rx_ns) = hemisphere(request.rx.lat, 'N', 'S');
    let (rx_lon, rx_ew) = hemisphere(request.rx.lon, 'E', 'W');
    let mut frequency_card = String::from("FREQUENCY ");
    for i in 0..MAX_FREQUENCIES {
        frequency_card.push_str(&format!("{:5.2}", frequencies.get(i).copied().unwrap_or(0.0)));
    }

    let cards = [
        "COMMENT    Any VOACAP default cards may be placed in the file: VOACAP.DEF".to_string(),
        "LINEMAX      55       number of lines-per-page".to_string(),
        format!("COEFFS    {coefficients}"),
        "TIME          1   24    1    1".to_string(),
        format!("MONTH     {:5}{:5.2}", request.year, request.month as f64),
        format!("SUNSPOT   {:4.0}.", request.ssn),
        format!(
            "LABEL     {:<w$.w$}{:<w$.w$}",
            request.tx_name,
            request.rx_name,
            w = LABEL_WIDTH
        ),
        format!(
            "CIRCUIT   {tx_lat:5.2}{tx_ns}{tx_lon:9.2}{tx_ew}{rx_lat:9.2}{rx_ns}{rx_lon:9.2}{rx_ew}  {path}{:6}",
            0
        ),
        format!(
            "SYSTEM    {:4.0}.{:4.0}.{:5.2}{:4.0}.{:5.1}{:5.2}{:5.2}",
            1.0,
            request.rx_noise_db,
            request.min_angle_deg,
            request.required_reliability_pct,
            request.required_snr_db_hz,
            3.0,
            0.1
        ),
        "FPROB      1.00 1.00 1.00 0.00".to_string(),
        // For a transmit isotrope the gain travels in the design-frequency
        // field; for a receive one, in the last field.
        antenna_card(1, &request.tx_antenna, request.tx_antenna.gain_dbi, request.tx_power_watts / 1000.0)?,
        antenna_card(2, &request.rx_antenna, 0.0, request.rx_antenna.gain_dbi)?,
        frequency_card,
        "METHOD       30    0".to_string(),
        "EXECUTE".to_string(),
        "QUIT".to_string(),
    ];

    let mut deck = String::new();
    for card in cards {
        deck.push_str(card.trim_end());
        deck.push('\n');
    }
    Ok(deck)
}

fn hemisphere(degrees: f64, positive: char, negative: char) -> (f64, char) {
    if degrees < 0.0 { (-degrees, negative) } else { (degrees, positive) }
}

fn antenna_card(index: u32, antenna: &Antenna, design: f64, last: f64) -> Result<String, String> {
    if antenna.file.len() > ANTENNA_FILE_WIDTH {
        return Err(format!(
            "antenna file name '{}' is longer than {ANTENNA_FILE_WIDTH} characters",
            antenna.file
        ));
    }
    Ok(format!(
        "ANTENNA   {index:5}{index:5}{:5}{:5}{design:10.3}[{:<w$}]{:5.1}{last:10.4}",
        2,
        30,
        antenna.file,
        antenna.bearing_deg,
        w = ANTENNA_FILE_WIDTH
    ))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::geo::LatLon;

    /// The request behind `tests/engine/cases/test01.dat`.
    pub(crate) fn reference_request() -> PredictionRequest {
        PredictionRequest {
            tx_name: "TANGIER, Morocco".into(),
            rx_name: "BELGRADE".into(),
            tx: LatLon { lat: 35.80, lon: -5.90 },
            rx: LatLon { lat: 44.90, lon: 20.50 },
            path: PathKind::Short,
            year: 1994,
            month: 6,
            ssn: 100.0,
            frequencies_mhz: vec![6.07, 7.20, 9.70, 11.85, 13.70, 15.35, 17.73, 21.65, 25.89],
            tx_power_watts: 500_000.0,
            tx_antenna: Antenna { file: "samples/sample.00".into(), bearing_deg: 90.0, gain_dbi: 10.0 },
            rx_antenna: Antenna { file: "samples/sample.00".into(), bearing_deg: 270.0, gain_dbi: 20.0 },
            rx_noise_db: 145.0,
            min_angle_deg: 0.1,
            required_reliability_pct: 90.0,
            required_snr_db_hz: 73.0,
            coefficients: Coefficients::Ccir,
        }
    }

    #[test]
    fn reproduces_reference_deck() {
        let expected = include_str!("../../../tests/engine/cases/test01.dat").replace("\r\n", "\n");
        assert_eq!(write_deck(&reference_request()).unwrap(), expected);
    }

    #[test]
    fn writes_southern_and_eastern_hemispheres_and_long_path() {
        let mut request = reference_request();
        request.tx = LatLon { lat: -33.87, lon: 151.21 };
        request.path = PathKind::Long;
        let deck = write_deck(&request).unwrap();
        assert!(deck.contains("CIRCUIT   33.87S   151.21E    44.90N    20.50E  L     0\n"), "{deck}");
    }

    #[test]
    fn rejects_out_of_range_input() {
        let mut request = reference_request();
        request.frequencies_mhz = vec![1.84];
        assert!(write_deck(&request).unwrap_err().contains("2-30 MHz"));

        let mut request = reference_request();
        request.frequencies_mhz = vec![7.1; 12];
        assert!(write_deck(&request).is_err());

        let mut request = reference_request();
        request.tx_antenna.file = "a/very/long/antenna/file/name.voa".into();
        assert!(write_deck(&request).is_err());
    }
}
