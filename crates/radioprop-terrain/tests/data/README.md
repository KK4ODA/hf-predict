# Test GeoTIFFs

What the tests feed the GeoTIFF reader and the store.

**They are built, not taken from Copernicus.** `../../tools/make_fixtures.py` writes them from the
TIFF 6.0 and GeoTIFF 1.1 formats and TIFF Technical Note 3's floating-point predictor, written
from those documents and not from the crate's reader. Their heights are
h(r, c) = ((37 r + 11 c) mod 97) / 2 − 20.25 metres, which the tests compute again.

| File | Stands for |
|---|---|
| `copernicus-like.tif` | A Copernicus DEM tile as the AWS copies are written: little-endian, tiled (8 by 8, the last column and row of tiles partial), DEFLATE with the floating-point predictor, *pixel is point*, laid out as tile N33W085 at 20 by 12 samples |
| `strips-big-endian.tif` | The other ways a TIFF can be: big-endian, strips of 5 rows (the last one 2), uncompressed, no predictor, *pixel is area* |

The real tiles are read by an ignored test: set `RADIOPROP_GLO90` and `RADIOPROP_GLO30` to the
GeoTIFFs of tile N33W085 (`Source::url` gives where to fetch them) and run
`cargo test -p radioprop-terrain --release -- --ignored --nocapture`. It checks Stone Mountain's
summit and the Chattahoochee at Atlanta, and prints the sizes and times. On 2026-11-21: GLO-90
1 200 × 1 200 read in 0.04 s, kept as 1.3 MB (5.4 MB fetched), Stone Mountain 501.1 m; GLO-30
3 600 × 3 600 read in 0.24 s, kept as 9.9 MB (45.5 MB fetched), Stone Mountain 504.2 m (its
bare granite summit is 513 m).
