# Winlink PROPAGATION catalog reply fixtures

Real replies to a Winlink catalog request, received 2026-10-04 and stored by Winlink Express as one MIME file per message.

| File | Catalog ID requested | Source product |
|---|---|---|
| `wwv.mime` | `PROP_WWV` | https://services.swpc.noaa.gov/text/wwv.txt |
| `sgas.mime` | `PROP_SGAS` (and `PROP_RSGA`, which returned the same product) | https://services.swpc.noaa.gov/text/sgas.txt |
| `3-day-forecast.mime` | `PROP3DNOAA` | https://services.swpc.noaa.gov/text/3-day-forecast.txt |
| `27-day-outlook.mime` | `PROP.27DO` | https://services.swpc.noaa.gov/text/27-day-outlook.txt |

## Changes from the originals

- The recipient callsign is replaced with `N0CALL`.
- The `Message-ID` is replaced with a placeholder.

Everything else is as stored, including line endings and quoted-printable encoding. `.gitattributes` stops git from normalising these files.

## Format notes for the parser

- Sender is `SERVICE@winlink.org`; subject is `INQUIRY - <source URL>`.
- The body is `text/plain`, ISO-8859-1, quoted-printable (soft line breaks, `=20`, `=3D`).
- The NOAA text starts with `:Product:` and `:Issued:` lines.
- A line of `=====` followed by a Winlink footer ends the body and is not part of the product.
- Missing values appear as `?` and `???`.

The NOAA text is US Government work in the public domain.
