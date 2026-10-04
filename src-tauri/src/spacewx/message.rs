//! Finds NOAA products inside whatever the operator hands over: the plain
//! product text, a Winlink reply as stored by a mail client, or several of
//! either pasted together.

const PRODUCT_HEADER: &str = ":Product:";
/// Winlink appends its footer after a line of equals signs.
const WINLINK_FOOTER: &str = "=====";

/// The products found in `raw`, each as normalised text starting at its
/// `:Product:` line. Normalised means LF line endings, no trailing spaces and
/// no trailing blank lines, so the same product is the same text however it
/// arrived.
pub fn extract_products(raw: &str) -> Vec<String> {
    let text = if raw.to_ascii_lowercase().contains("quoted-printable") {
        decode_quoted_printable(raw)
    } else {
        raw.to_string()
    };

    let mut products: Vec<Vec<&str>> = Vec::new();
    let mut inside = false;
    for line in text.lines().map(str::trim_end) {
        if line.starts_with(PRODUCT_HEADER) {
            products.push(Vec::new());
            inside = true;
        } else if line.starts_with(WINLINK_FOOTER) || line.starts_with("--") {
            // A footer or a MIME boundary ends the product.
            inside = false;
        }
        if inside {
            if let Some(product) = products.last_mut() {
                product.push(line);
            }
        }
    }

    products
        .into_iter()
        .map(|mut lines| {
            while lines.last().is_some_and(|l| l.is_empty()) {
                lines.pop();
            }
            lines.join("\n") + "\n"
        })
        .collect()
}

/// Whether the text is a Winlink reply rather than the bare product.
pub fn is_winlink_message(raw: &str) -> bool {
    raw.to_ascii_lowercase().contains("winlink.org")
}

/// Undoes quoted-printable encoding: `=XX` is a byte, `=` at a line end
/// joins the line to the next. Bytes are read as ISO-8859-1.
fn decode_quoted_printable(text: &str) -> String {
    let unfolded = text.replace("=\r\n", "").replace("=\n", "");
    let mut decoded = String::with_capacity(unfolded.len());
    let mut chars = unfolded.chars();
    while let Some(c) = chars.next() {
        if c != '=' {
            decoded.push(c);
            continue;
        }
        let pair: String = chars.clone().take(2).collect();
        match u8::from_str_radix(&pair, 16) {
            Ok(byte) if pair.len() == 2 => {
                decoded.push(byte as char);
                chars.nth(1);
            }
            _ => decoded.push('='),
        }
    }
    decoded
}

#[cfg(test)]
mod tests {
    use super::*;

    const WINLINK_SGAS: &str = include_str!("../../../tests/fixtures/winlink/sgas.mime");
    const WINLINK_THREE_DAY: &str = include_str!("../../../tests/fixtures/winlink/3-day-forecast.mime");
    const WINLINK_OUTLOOK: &str = include_str!("../../../tests/fixtures/winlink/27-day-outlook.mime");
    const NOAA_SGAS: &str = include_str!("../../../tests/fixtures/noaa/sgas.txt");
    const NOAA_THREE_DAY: &str = include_str!("../../../tests/fixtures/noaa/3-day-forecast.txt");
    const NOAA_OUTLOOK: &str = include_str!("../../../tests/fixtures/noaa/27-day-outlook.txt");

    /// The same NOAA issue must come out as identical text whether it was
    /// fetched from NOAA or unwrapped from a Winlink reply.
    #[test]
    fn winlink_reply_and_noaa_text_normalise_to_the_same_product() {
        for (winlink, noaa) in [
            (WINLINK_SGAS, NOAA_SGAS),
            (WINLINK_THREE_DAY, NOAA_THREE_DAY),
            (WINLINK_OUTLOOK, NOAA_OUTLOOK),
        ] {
            let from_winlink = extract_products(winlink);
            let from_noaa = extract_products(noaa);
            assert_eq!(from_winlink.len(), 1);
            assert_eq!(from_winlink, from_noaa);
        }
    }

    #[test]
    fn decodes_soft_line_breaks_and_escapes() {
        let products = extract_products(WINLINK_THREE_DAY);
        // The long "Prepared by" line was folded with a soft break in transit.
        assert!(products[0].contains("Space Weather Prediction Center\n"));
        // A trailing "=20" was an encoded space, removed with other trailing spaces.
        assert!(products[0].contains("00-03UT       3.00         3.33         3.33\n"));
        assert!(!products[0].contains("Thanks for using Winlink"));
    }

    #[test]
    fn finds_several_products_pasted_together() {
        let pasted = format!("{WINLINK_SGAS}\n\nsome notes\n{NOAA_OUTLOOK}");
        let products = extract_products(&pasted);
        assert_eq!(products.len(), 2);
        assert!(products[0].starts_with(":Product: Solar and Geophysical Activity Summary"));
        assert!(products[1].starts_with(":Product: 27-day Space Weather Outlook"));
    }

    #[test]
    fn text_without_a_product_gives_nothing() {
        assert!(extract_products("just a note").is_empty());
        assert!(extract_products("").is_empty());
    }

    #[test]
    fn recognises_winlink_replies() {
        assert!(is_winlink_message(WINLINK_SGAS));
        assert!(!is_winlink_message(NOAA_SGAS));
    }
}
