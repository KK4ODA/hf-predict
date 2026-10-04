//! Reads the text of a decoded FT8 or FT4 message: who sent it, to whom,
//! and any locator or report it carries. WSJT-X sends only the text.

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Kind {
    Cq,
    /// A reply or exchange that carries the sender's locator.
    Grid,
    Report,
    RogerReport,
    Rrr,
    Rr73,
    SeventyThree,
    /// Free text, contest and DXpedition forms, or anything unrecognised.
    Other,
}

impl Kind {
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Cq => "cq",
            Kind::Grid => "grid",
            Kind::Report => "report",
            Kind::RogerReport => "rogerReport",
            Kind::Rrr => "rrr",
            Kind::Rr73 => "rr73",
            Kind::SeventyThree => "73",
            Kind::Other => "other",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Ft8Text {
    pub kind: Kind,
    /// The transmitting station: the one this receiver actually heard.
    pub from: Option<String>,
    pub to: Option<String>,
    /// The sender's four-character locator.
    pub grid: Option<String>,
    pub report_db: Option<i32>,
}

const OTHER: Ft8Text = Ft8Text { kind: Kind::Other, from: None, to: None, grid: None, report_db: None };

/// A four-character Maidenhead locator. `RR73` has the same shape but is an
/// acknowledgement, never a locator.
fn is_grid(token: &str) -> bool {
    let b = token.as_bytes();
    token != "RR73"
        && b.len() == 4
        && (b'A'..=b'R').contains(&b[0])
        && (b'A'..=b'R').contains(&b[1])
        && b[2].is_ascii_digit()
        && b[3].is_ascii_digit()
}

/// A callsign, possibly with a prefix or suffix (`PJ4/K1ABC`, `K1ABC/P`), or
/// one in angle brackets, which is how WSJT-X shows a hashed callsign.
fn callsign(token: &str) -> Option<Option<String>> {
    if let Some(inner) = token.strip_prefix('<').and_then(|t| t.strip_suffix('>')) {
        // `<...>` is a hash the decoder could not resolve.
        return Some((inner != "...").then(|| inner.to_string()));
    }
    let plausible = (3..=11).contains(&token.len())
        && token.bytes().all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'/')
        && token.bytes().any(|b| b.is_ascii_digit())
        && token.bytes().any(|b| b.is_ascii_uppercase())
        && !is_grid(token)
        && token != "RR73";
    plausible.then(|| Some(token.to_string()))
}

/// `-12`, `+05`
fn report(token: &str) -> Option<i32> {
    let valid = token.len() == 3 && (token.starts_with('+') || token.starts_with('-'));
    valid.then(|| token.parse().ok()).flatten()
}

pub fn parse(text: &str) -> Ft8Text {
    let tokens: Vec<&str> = text.split_whitespace().collect();

    if tokens.first() == Some(&"CQ") {
        // CQ [modifier] CALL [GRID]
        let (grid, rest) = match tokens.split_last() {
            Some((last, rest)) if is_grid(last) => (Some(last.to_string()), rest),
            _ => (None, &tokens[..]),
        };
        return match rest.last().and_then(|t| callsign(t)) {
            Some(from) if rest.len() >= 2 => Ft8Text { kind: Kind::Cq, from, to: None, grid, report_db: None },
            _ => OTHER,
        };
    }

    let (Some(to), Some(from)) = (
        tokens.first().and_then(|t| callsign(t)),
        tokens.get(1).and_then(|t| callsign(t)),
    ) else {
        return OTHER;
    };
    let exchange = Ft8Text { kind: Kind::Other, from, to, grid: None, report_db: None };
    match tokens.get(2).copied() {
        Some("RRR") => Ft8Text { kind: Kind::Rrr, ..exchange },
        Some("RR73") => Ft8Text { kind: Kind::Rr73, ..exchange },
        Some("73") => Ft8Text { kind: Kind::SeventyThree, ..exchange },
        Some(token) if is_grid(token) => {
            Ft8Text { kind: Kind::Grid, grid: Some(token.to_string()), ..exchange }
        }
        // Contest form: CALL CALL R GRID
        Some("R") if tokens.get(3).is_some_and(|t| is_grid(t)) => {
            Ft8Text { kind: Kind::Grid, grid: Some(tokens[3].to_string()), ..exchange }
        }
        Some(token) if report(token).is_some() => {
            Ft8Text { kind: Kind::Report, report_db: report(token), ..exchange }
        }
        Some(token) if token.strip_prefix('R').and_then(report).is_some() => Ft8Text {
            kind: Kind::RogerReport,
            report_db: token.strip_prefix('R').and_then(report),
            ..exchange
        },
        _ => exchange,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn check(text: &str, kind: Kind, from: Option<&str>, to: Option<&str>, grid: Option<&str>) {
        let parsed = parse(text);
        assert_eq!(
            (parsed.kind, parsed.from.as_deref(), parsed.to.as_deref(), parsed.grid.as_deref()),
            (kind, from, to, grid),
            "{text}"
        );
    }

    #[test]
    fn reads_cq_calls() {
        check("CQ K1ABC FN42", Kind::Cq, Some("K1ABC"), None, Some("FN42"));
        check("CQ DX K1ABC FN42", Kind::Cq, Some("K1ABC"), None, Some("FN42"));
        check("CQ POTA W9XYZ/P EN37", Kind::Cq, Some("W9XYZ/P"), None, Some("EN37"));
        check("CQ 290 OH2ZZ KP20", Kind::Cq, Some("OH2ZZ"), None, Some("KP20"));
        check("CQ PJ4/K1ABC", Kind::Cq, Some("PJ4/K1ABC"), None, None);
        check("CQ <PJ4/K1ABC>", Kind::Cq, Some("PJ4/K1ABC"), None, None);
    }

    #[test]
    fn the_sender_of_an_exchange_is_the_second_callsign() {
        check("K1ABC W9XYZ EN37", Kind::Grid, Some("W9XYZ"), Some("K1ABC"), Some("EN37"));
        check("K1ABC W9XYZ -12", Kind::Report, Some("W9XYZ"), Some("K1ABC"), None);
        check("W9XYZ K1ABC R+05", Kind::RogerReport, Some("K1ABC"), Some("W9XYZ"), None);
        check("K1ABC W9XYZ RRR", Kind::Rrr, Some("W9XYZ"), Some("K1ABC"), None);
        check("K1ABC W9XYZ 73", Kind::SeventyThree, Some("W9XYZ"), Some("K1ABC"), None);
        check("K1ABC/R W9XYZ/R R FN42", Kind::Grid, Some("W9XYZ/R"), Some("K1ABC/R"), Some("FN42"));
        assert_eq!(parse("K1ABC W9XYZ -12").report_db, Some(-12));
        assert_eq!(parse("W9XYZ K1ABC R+05").report_db, Some(5));
    }

    #[test]
    fn rr73_is_never_a_locator() {
        check("K1ABC W9XYZ RR73", Kind::Rr73, Some("W9XYZ"), Some("K1ABC"), None);
        check("CQ K1ABC RR73", Kind::Other, None, None, None);
    }

    #[test]
    fn hashed_callsigns_are_kept_when_known() {
        check("<PJ4/K1ABC> W9XYZ -08", Kind::Report, Some("W9XYZ"), Some("PJ4/K1ABC"), None);
        check("K1ABC <...> RR73", Kind::Rr73, None, Some("K1ABC"), None);
    }

    #[test]
    fn free_text_and_unknown_forms_name_no_sender() {
        for text in ["TNX BOB 73 GL", "K1ABC RR73; W9XYZ <KH1/KH7Z> -08", "", "CQ", "TU; K1ABC W9XYZ 579 MA"] {
            check(text, Kind::Other, None, None, None);
        }
        // Two callsigns with an exchange this parser does not model still name the sender.
        check("K1ABC W9XYZ 6A WI", Kind::Other, Some("W9XYZ"), Some("K1ABC"), None);
    }
}
