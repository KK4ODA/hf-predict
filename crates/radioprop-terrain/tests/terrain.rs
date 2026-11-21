// SPDX-License-Identifier: MIT OR Apache-2.0
//! radioprop-terrain against cases worked out apart from it: GeoTIFFs written by
//! `tools/make_fixtures.py` from the formats' documents, heights that bilinear reading must give
//! exactly (planes), and ITU-R P.526-16's equations worked step by step for chosen paths.

use std::path::PathBuf;

use radioprop_geo::{distance_km, intermediate, LatLon};
use radioprop_terrain::budget::{self, Ends};
use radioprop_terrain::geotiff::Raster;
use radioprop_terrain::p526::{self, Ground, Polarization, Radio};
use radioprop_terrain::path::{self, Class, K_STANDARD};
use radioprop_terrain::profile::{self, Profile};
use radioprop_terrain::store::{Have, Height, Source, Store, TileId};
use radioprop_terrain::tile::{self, TileFile};

fn data(name: &str) -> Vec<u8> {
    std::fs::read(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/data").join(name))
        .expect("a fixture")
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("radioprop-terrain-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

/// The fixtures' heights, as `make_fixtures.py` writes them.
fn fixture_h(r: usize, c: usize) -> f32 {
    ((37 * r + 11 * c) % 97) as f32 / 2.0 - 20.25
}

fn close(a: f64, b: f64, within: f64) -> bool {
    (a - b).abs() <= within
}

#[test]
fn reads_a_geotiff_laid_out_as_copernicus_writes_them() {
    let r = Raster::read(&data("copernicus-like.tif")).expect("read");
    assert_eq!((r.width, r.height), (20, 12));
    // pixel is point: the first sample is the tiepoint itself
    assert_eq!((r.west, r.north), (-85.0, 34.0));
    assert!(close(r.dlon, 1.0 / 20.0, 1e-15) && close(r.dlat, 1.0 / 12.0, 1e-15));
    for row in 0..12 {
        for col in 0..20 {
            assert_eq!(r.heights[row * 20 + col], fixture_h(row, col), "({row}, {col})");
        }
    }
}

#[test]
fn reads_big_endian_strips_and_pixel_is_area() {
    let r = Raster::read(&data("strips-big-endian.tif")).expect("read");
    assert_eq!((r.width, r.height), (7, 12));
    // pixel is area: the tiepoint is the raster's corner, half a sample north-west of the first
    assert!(close(r.west, -85.0, 1e-12) && close(r.north, 34.0, 1e-12), "{} {}", r.west, r.north);
    for row in 0..12 {
        for col in 0..7 {
            assert_eq!(r.heights[row * 7 + col], fixture_h(row, col), "({row}, {col})");
        }
    }
}

#[test]
fn refuses_what_it_does_not_read() {
    assert!(Raster::read(b"not a tiff at all").unwrap_err().contains("not a TIFF"));
    let mut bytes = data("copernicus-like.tif");
    // cut the file short: the tiles are gone
    bytes.truncate(400);
    assert!(Raster::read(&bytes).is_err());
}

/// A raster for tile `t` over a plane h = 100 + 40 (lat − t.lat) − 25 (lon − t.lon) + extra, at
/// `n` samples a degree: bilinear reading of a plane is exact, wherever it falls.
fn plane(t: TileId, n: usize, extra: f64) -> Raster {
    let (north, west) = (f64::from(t.lat + 1), f64::from(t.lon));
    let d = 1.0 / n as f64;
    let mut heights = Vec::with_capacity(n * n);
    for row in 0..n {
        for col in 0..n {
            let (lat, lon) = (north - row as f64 * d, west + col as f64 * d);
            heights.push(plane_h(lat, lon, extra) as f32);
        }
    }
    Raster { width: n, height: n, west, north, dlon: d, dlat: d, heights }
}

fn plane_h(lat: f64, lon: f64, extra: f64) -> f64 {
    100.0 + 40.0 * (lat - 33.0) - 25.0 * (lon + 85.0) + extra
}

#[test]
fn a_tile_kept_on_disk_gives_back_its_heights_to_the_half_metre() {
    let dir = scratch("tile");
    let mut r = plane(TileId { lat: 33, lon: -85 }, 600, 0.0);
    // the extremes a 16-bit half-metre holds, a height between, and no height at all
    r.heights[0] = 8848.9;
    r.heights[1] = -430.26;
    r.heights[2] = f32::NAN;
    r.heights[599] = 16_383.0;
    let path = dir.join("t.rpt");
    let size = tile::write(&path, &r, "test", "plane").expect("written");
    assert!(size > 0 && path.exists() && !path.with_extension("part").exists());
    let mut f = TileFile::open(&path).expect("opened");
    assert_eq!((f.header.width, f.header.height, f.header.block), (600, 600, 256));
    for &(row, col) in &[(0, 0), (0, 1), (0, 2), (0, 599), (255, 255), (256, 256), (599, 599), (300, 511)] {
        let (k, i) = f.locate(row, col);
        let v = f.block(k).expect("block")[i];
        let want = r.heights[row * 600 + col];
        if want.is_nan() {
            assert_eq!(v, tile::NO_HEIGHT);
        } else {
            assert!(close(f64::from(v) / 2.0, f64::from(want), 0.25), "({row}, {col}): {v} for {want}");
        }
    }
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn heights_are_read_bilinearly_across_tile_edges() {
    let dir = scratch("store");
    let store = Store::new(&dir, Source::Glo90);
    let tiles = [TileId { lat: 33, lon: -85 }, TileId { lat: 33, lon: -84 }, TileId { lat: 32, lon: -85 }, TileId { lat: 32, lon: -84 }];
    for t in tiles {
        assert_eq!(store.have(t), Have::Missing);
        store.put_raster(t, &plane(t, 60, 0.0)).expect("kept");
        assert_eq!(store.have(t), Have::Held);
    }
    // inside a tile, on its edges, across its south and east edges, and at the corner of four
    let points = [
        (33.5, -84.5),
        (33.512_3, -84.987_6),
        (34.0, -85.0),
        (33.0, -84.4),
        (33.004, -84.2),
        (33.5, -84.004),
        (33.003, -84.002),
        (33.0, -84.0),
        (32.999, -83.999),
    ];
    for (lat, lon) in points {
        match store.height(LatLon { lat, lon }).expect("read") {
            Height::Metres(h) => {
                assert!(close(h, plane_h(lat, lon, 0.0), 0.3), "{lat}, {lon}: {h} for {}", plane_h(lat, lon, 0.0));
            }
            Height::Missing(t) => panic!("{lat}, {lon}: {t:?} missing"),
        }
    }
    // a tile not fetched is said, and one the data set does not have is the sea
    assert_eq!(store.height(LatLon { lat: 35.5, lon: -84.5 }).unwrap(), Height::Missing(TileId { lat: 35, lon: -85 }));
    let sea = TileId { lat: 31, lon: -80 };
    store.put_absent(sea).expect("marked");
    assert_eq!(store.have(sea), Have::Absent);
    assert_eq!(store.height(LatLon { lat: 31.5, lon: -79.5 }).unwrap(), Height::Metres(0.0));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_tile_must_be_the_one_named() {
    let dir = scratch("named");
    let store = Store::new(&dir, Source::Glo90);
    // the fixture is laid out as tile N33W085
    store.put(TileId { lat: 33, lon: -85 }, &data("copernicus-like.tif")).expect("kept");
    let wrong = store.put(TileId { lat: 34, lon: -85 }, &data("copernicus-like.tif")).unwrap_err();
    assert!(wrong.contains("north-west corner"), "{wrong}");
    // its samples come back where they were, kept to the half-metre: row 3, column 7 is
    // 25.25 m, kept as 25.5 (51 half-metres, rounded away from zero)
    match store.height(LatLon { lat: 34.0 - 3.0 / 12.0, lon: -85.0 + 7.0 / 20.0 }).unwrap() {
        Height::Metres(h) => assert!(close(h, 25.5, 1e-9), "{h}"),
        other => panic!("{other:?}"),
    }
    assert_eq!(fixture_h(3, 7), 25.25);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn copernicus_names_its_tiles_by_their_south_west_corner() {
    let t = TileId::of(LatLon { lat: 33.75, lon: -84.39 });
    assert_eq!(t, TileId { lat: 33, lon: -85 });
    assert_eq!(t.name(), "N33W085");
    // a tile's north edge is its own, its south edge the tile below's
    assert_eq!(TileId::of(LatLon { lat: 34.0, lon: -84.39 }), t);
    assert_eq!(TileId::of(LatLon { lat: 33.0, lon: -84.39 }), TileId { lat: 32, lon: -85 });
    assert_eq!(Source::Glo30.key(t), "Copernicus_DSM_COG_10_N33_00_W085_00_DEM");
    assert_eq!(
        Source::Glo90.url(TileId { lat: -10, lon: 7 }),
        "https://copernicus-dem-90m.s3.amazonaws.com/Copernicus_DSM_COG_30_S10_00_E007_00_DEM/Copernicus_DSM_COG_30_S10_00_E007_00_DEM.tif"
    );
    assert_eq!(TileId { lat: 0, lon: 179 }.east(), TileId { lat: 0, lon: -180 });
}

#[test]
fn a_profile_follows_the_great_circle_at_the_datas_spacing() {
    let dir = scratch("profile");
    let store = Store::new(&dir, Source::Glo90);
    let (from, to) = (LatLon { lat: 33.80, lon: -84.40 }, LatLon { lat: 32.70, lon: -83.60 });
    let wanted = profile::tiles(from, to);
    assert!(wanted.contains(&TileId { lat: 33, lon: -85 }) && wanted.contains(&TileId { lat: 32, lon: -84 }), "{wanted:?}");
    match profile::profile(&store, from, to) {
        Err(Ok(missing)) => assert!(!missing.is_empty()),
        other => panic!("{other:?}"),
    }
    for t in &wanted {
        store.put_raster(*t, &plane(*t, 60, 0.0)).expect("kept");
    }
    let p: Profile = profile::profile(&store, from, to).expect("a profile");
    assert!(close(p.length_km, distance_km(from, to), 1e-9));
    assert_eq!(p.d_km.len(), profile::samples(p.length_km, Source::Glo90));
    // no two samples further apart than the data's own (3″, about 93 m)
    assert!(p.d_km.windows(2).all(|w| w[1] - w[0] <= 0.093));
    for (i, (&d, &h)) in p.d_km.iter().zip(&p.h_m).enumerate().step_by(97) {
        let at = intermediate(from, to, d / p.length_km);
        assert!(close(h, plane_h(at.lat, at.lon, 0.0), 0.3), "{i}: {h}");
    }
    let _ = std::fs::remove_dir_all(dir);
}

// ITU-R P.526-16, worked by hand for these cases (the steps are in the comments)

#[test]
fn knife_edge_loss_follows_equation_31() {
    // J(0) = 6.9 + 20 log(√1.01 − 0.1) = 6.03; J(1) = 6.9 + 20 log(√1.81 + 0.9) = 13.93
    assert!(close(p526::j(0.0), 6.0329, 1e-4));
    assert!(close(p526::j(1.0), 13.9257, 1e-4));
    // equation 31 holds for ν above −0.78, where it has come down to zero: J(−0.7799) = 0.0047;
    // at −0.78 and below, J is zero (§ 4.5.1, after equation 52)
    assert!(close(p526::j(-0.7799), 0.00469, 1e-5), "{}", p526::j(-0.7799));
    assert_eq!(p526::j(-0.78), 0.0);
    assert_eq!(p526::j(-0.8), 0.0);
}

#[test]
fn a_smooth_earth_beyond_the_horizon_is_section_3_1_1s_first_term() {
    // 145 MHz, both antennas 10 m, 50 km, k = 4/3, land (εr 22, σ 0.003), vertical:
    // K_H = 0.36 (ae f)^(-1/3) [(ε−1)² + (18000 σ/f)²]^(-1/4) = 7.328e-4, K_V = K_H √(ε² + …) = 0.016124
    // β = 0.999247; X = 2.188 β f^(1/3) ae^(-2/3) d = 1.37948, F = −20 log X − 5.6488 X^1.425 = −11.7284
    // Y = 9.575e-3 β f^(2/3) ae^(-1/3) h = 0.12942, B = βY, G = 20 log(B + 0.1 B³) = −17.7519
    // (the floor 2 + 20 log K is −33.85); the horizon is √(2 ae) (√h1 + √h2) = 26.07 km
    let ae = K_STANDARD * 6371.0;
    let e = p526::first_term(50.0, 10.0, 10.0, 145.0, ae, Ground::LAND, Polarization::Vertical);
    assert!(close(e, -47.2321, 1e-3), "{e}");
    let a = p526::spherical_earth(50.0, 10.0, 10.0, 145.0, ae, Ground::LAND, Polarization::Vertical);
    assert!(close(a, 47.2321, 1e-3), "{a}");
    // 30 m and 100 m, 80 km: X = 2.20717, F = 11 + 10 log X − 17.6 X = −24.4078,
    // G(30) = 20 log(B + 0.1 B³) = −8.0942, G(100): B > 2, 17.6 √(B − 1.1) − 5 log(B − 1.1) − 8 = 3.5769
    let e = p526::first_term(80.0, 30.0, 100.0, 145.0, ae, Ground::LAND, Polarization::Vertical);
    assert!(close(e, -28.9251, 1e-3), "{e}");
}

#[test]
fn a_smooth_earth_inside_the_horizon_is_interpolated_by_section_3_2() {
    // 145 MHz, 10 m and 30 m, 30 km (the horizon is 35.61 km):
    // c = −0.5, m = d²/(4 ae (h1+h2)) = 0.662180, b = −0.313029, d1 = 10 304.56 m, d2 = 19 695.44 m,
    // h = 4.9238 m, h_req = 0.552 √(d1 d2 λ / d) = 65.2833 m, a_em = 6 028.86 km,
    // A_h = 30.1458 dB, A = (1 − h / h_req) A_h = 27.8721 dB
    let ae = K_STANDARD * 6371.0;
    let a = p526::spherical_earth(30.0, 10.0, 30.0, 145.0, ae, Ground::LAND, Polarization::Vertical);
    assert!(close(a, 27.8721, 1e-3), "{a}");
    // well clear (a short path between high antennas): no loss
    assert_eq!(p526::spherical_earth(5.0, 50.0, 50.0, 145.0, ae, Ground::LAND, Polarization::Vertical), 0.0);
    // the two branches meet at the horizon
    let dlos = (2.0 * ae * 1000.0).sqrt() * (10f64.sqrt() + 30f64.sqrt()) / 1000.0;
    let inside = p526::spherical_earth(dlos - 1e-6, 10.0, 30.0, 145.0, ae, Ground::LAND, Polarization::Vertical);
    let beyond = p526::spherical_earth(dlos + 1e-6, 10.0, 30.0, 145.0, ae, Ground::LAND, Polarization::Vertical);
    assert!(close(inside, beyond, 0.01), "{inside} {beyond}");
}

fn vhf(re_km: f64) -> Radio {
    Radio { f_mhz: 145.0, re_km, ground: Ground::LAND, pol: Polarization::Vertical }
}

/// A profile `length_km` long, sampled every `step_km`, flat at `ground_m` but for an `obstacle`
/// (distance, height) standing on one sample.
fn flat(length_km: f64, step_km: f64, ground_m: f64, obstacle: Option<(f64, f64)>) -> (Vec<f64>, Vec<f64>) {
    let n = (length_km / step_km).round() as usize + 1;
    let d: Vec<f64> = (0..n).map(|i| length_km * i as f64 / (n - 1) as f64).collect();
    let h = d
        .iter()
        .map(|&x| match obstacle {
            Some((at, top)) if (x - at).abs() < step_km / 2.0 => top,
            _ => ground_m,
        })
        .collect();
    (d, h)
}

#[test]
fn bullington_on_one_knife_edge_is_its_knife_edge_loss() {
    // 145 MHz (λ 2.06753 m), 20 km, flat ground at 0 under both antennas (10 m), a 60 m edge at
    // 10 km, the Earth's curvature taken away (an enormous radius): the Bullington point is the
    // edge, 50 m above the ray, ν = 50 √(0.002 × 20 / (λ × 10 × 10)) = 0.69546 (equation 26's
    // ν for d1 = d2 = 10 km), J = 11.8069, L_b = J + (1 − e^(−J/6)) (10 + 0.02 × 20) = 20.7534
    let (d, h) = flat(20.0, 0.1, 0.0, Some((10.0, 60.0)));
    let b = p526::bullington(&d, &h, 10.0, 10.0, 145.0, 1e12);
    assert!(!b.line_of_sight);
    assert!(close(b.at_km, 10.0, 1e-9), "{}", b.at_km);
    assert!(close(b.nu, 0.69546, 1e-5), "{}", b.nu);
    assert!(close(b.knife_db, 11.8069, 1e-3) && close(b.loss_db, 20.7534, 1e-3), "{b:?}");
}

#[test]
fn a_smooth_path_gives_the_spherical_earths_loss() {
    // § 4.5: "for a completely smooth path, the result will be the same as the spherical Earth
    // model". Sea-level ground, 10 m antennas, 50 km at 145 MHz: 47.23 dB, as worked above.
    let re = K_STANDARD * 6371.0;
    let (d, h) = flat(50.0, 0.03, 0.0, None);
    let g = p526::general_path(&d, &h, 10.0, 10.0, vhf(re));
    assert!(close(g.loss_db, 47.2321, 1e-3), "{g:?}");
    assert!(close(g.spherical_db, g.loss_db, 1e-9));
    // Over the horizon one sharp ridge lowers the loss: obstacle gain. The fitted surface comes
    // down under the ridge (equations 58 to 64: h_stip = 0.144 m, h_obs = 140 m, g_t = 0.6, so
    // h_st = −83.86 m and h_ts′ = 93.86 m), and the knife edge loses less than the sphere did.
    let (d, h) = flat(50.0, 0.03, 0.0, Some((20.0, 150.0)));
    let ridge = p526::general_path(&d, &h, 10.0, 10.0, vhf(re));
    assert!(close(ridge.effective_m.0, 93.86, 0.05), "{:?}", ridge.effective_m);
    assert!(ridge.loss_db < g.loss_db, "{ridge:?}");
    // On a 10 km path between 60 m masts the smooth Earth costs nothing: at mid-path the ray
    // clears the bulge by 58.5 m, over 60 % of the 71.8 m first Fresnel zone. A 120 m ridge at
    // the sample nearest 4 km (3.994 km) cuts the ray: with its bulge it stands 121.41 m, S_tim =
    // 15.376, S_rim = 10.225, the Bullington point is the ridge (d_b = 3.994 km), ν_b = 1.23323,
    // J = 15.347 dB and L_b = J + (1 − e^(−J/6)) (10 + 0.2) = 24.757 dB
    let (d, h) = flat(10.0, 0.03, 0.0, None);
    let open = p526::general_path(&d, &h, 60.0, 60.0, vhf(re));
    assert_eq!(open.loss_db, 0.0, "{open:?}");
    let (d, h) = flat(10.0, 0.03, 0.0, Some((4.0, 120.0)));
    let cut = p526::general_path(&d, &h, 60.0, 60.0, vhf(re));
    assert!(close(cut.actual.nu, 1.23323, 1e-5) && close(cut.actual.loss_db, 24.7570, 1e-3), "{cut:?}");
    assert!(cut.loss_db >= cut.actual.loss_db);
}

#[test]
fn raising_an_antenna_never_adds_loss() {
    let re = K_STANDARD * 6371.0;
    // rolling ground with a ridge two thirds of the way
    let n = 1001;
    let d: Vec<f64> = (0..n).map(|i| 30.0 * i as f64 / (n - 1) as f64).collect();
    let h: Vec<f64> = d
        .iter()
        .map(|&x| 250.0 + 30.0 * (x * 0.9).sin() + 160.0 * (-((x - 20.0) / 1.5).powi(2)).exp())
        .collect();
    let mut before = f64::INFINITY;
    for agl in [2.0, 5.0, 10.0, 20.0, 40.0, 80.0, 160.0] {
        let l = p526::general_path(&d, &h, agl, 15.0, vhf(re)).loss_db;
        assert!(l <= before + 1e-9, "{agl} m: {l} after {before}");
        before = l;
    }
    // from 2 m to 160 m the ridge (67 m above the ray even then) costs far less
    let two = p526::general_path(&d, &h, 2.0, 15.0, vhf(re)).loss_db;
    assert!(two - before > 10.0, "{two} at 2 m, {before} at 160 m");
}

#[test]
fn a_path_is_called_clear_partly_obstructed_or_blocked() {
    let flat_profile = |obstacle| {
        let (d, h) = flat(30.0, 0.03, 200.0, obstacle);
        Profile {
            from: LatLon { lat: 33.0, lon: -84.0 },
            to: LatLon { lat: 33.27, lon: -84.0 },
            length_km: 30.0,
            d_km: d,
            h_m: h,
            source: Source::Glo30,
        }
    };
    // the first Fresnel zone at mid-path: 17.3 √(15 × 15 / (0.145 × 30)) = 124.42 m; the bulge
    // there 15 × 15 / (2 × 8494.67) km = 13.24 m
    let high = path::analyse(&flat_profile(None), 145.0, 150.0, 150.0, K_STANDARD);
    let mid = high.index_at(15.0);
    assert!(close(high.fresnel_m[mid], 124.42, 0.05), "{}", high.fresnel_m[mid]);
    assert!(close(high.ground_m[mid] - 200.0, 13.24, 0.01), "{}", high.ground_m[mid]);
    assert_eq!(high.class, Class::Clear);
    assert_eq!(high.words(), "Clear terrain profile");
    // 10 m masts on flat ground 30 km apart: the ray clears the bulge, but not by 60 % of the zone
    let low = path::analyse(&flat_profile(None), 145.0, 30.0, 30.0, K_STANDARD);
    assert!(matches!(low.class, Class::Partly { at_km } if close(at_km, 15.0, 0.5)), "{:?}", low.class);
    // a 90 m hill at 12 km stands 90 + bulge(12, 18) − 30 = 60 + 12.71 m above the ray
    let hill = path::analyse(&flat_profile(Some((12.0, 290.0))), 145.0, 30.0, 30.0, K_STANDARD);
    match hill.class {
        Class::Blocked { at_km, above_ray_m } => {
            assert!(close(at_km, 12.0, 0.03), "{at_km}");
            assert!(close(above_ray_m, 60.0 + 12.0 * 18.0 / (2.0 * K_STANDARD * 6371.0) * 1000.0, 0.01), "{above_ray_m}");
        }
        other => panic!("{other:?}"),
    }
    assert!(hill.words().starts_with("Blocked by terrain 73 m above the direct ray at 12.0 km"), "{}", hill.words());
    assert!(hill.diffraction.loss_db > low.diffraction.loss_db);
}

#[test]
fn the_link_budget_of_the_designs_example() {
    // § 8.6: 2 m, 40 km, a 50 W mobile at 0 dBi net, the gateway's antenna 6 dBi with 2 dB of
    // cable; free space 32.45 + 20 log 145 + 20 log 40 = 107.72 dB; through a 20 dB ridge the
    // gateway receives 46.99 (50 W) + 0 − 127.72 + 6 − 2 = −76.73 dBm
    let fspl = budget::fspl_db(145.0, 40.0);
    assert!(close(fspl, 107.719, 1e-3), "{fspl}");
    let ends = Ends { power_w: 50.0, tx_gain_dbi: 0.0, tx_feed_db: 0.0, rx_gain_dbi: 6.0, rx_feed_db: 2.0 };
    let b = budget::one_way(ends, fspl + 20.0, -110.0, 10.0);
    assert!(close(b.received_dbm, -76.729, 1e-3), "{b:?}");
    assert!(close(b.margin_db, 33.271, 1e-3));
    assert!(b.p_viable > 0.999);
    // Φ: half at zero margin; 97.5 % at 1.96 σ; and symmetric
    assert!(close(budget::phi(0.0), 0.5, 1e-7));
    assert!(close(budget::phi(1.959_964), 0.975, 1e-6));
    assert!(close(budget::phi(-1.0) + budget::phi(1.0), 1.0, 1e-7));
}

// The real data: set RADIOPROP_GLO90 (and RADIOPROP_GLO30) to the Copernicus GeoTIFF of tile
// N33W085 to run these (cargo test -p radioprop-terrain -- --ignored).

fn real(var: &str) -> Option<Vec<u8>> {
    std::env::var_os(var).map(|p| std::fs::read(p).expect("the tile named"))
}

#[test]
#[ignore = "needs a Copernicus tile: RADIOPROP_GLO90 or RADIOPROP_GLO30"]
fn reads_a_real_copernicus_tile() {
    for (var, source) in [("RADIOPROP_GLO90", Source::Glo90), ("RADIOPROP_GLO30", Source::Glo30)] {
        let Some(tif) = real(var) else { continue };
        let started = std::time::Instant::now();
        let r = Raster::read(&tif).expect("read");
        let read = started.elapsed();
        let n = (1.0 / source.dlat()).round() as usize;
        assert_eq!((r.width, r.height), (n, n));
        assert_eq!((r.west, r.north), (-85.0, 34.0));
        let dir = scratch(source.name());
        let store = Store::new(&dir, source);
        let t = TileId { lat: 33, lon: -85 };
        let size = store.put(t, &tif).expect("kept");
        let at = |lat, lon| match store.height(LatLon { lat, lon }).unwrap() {
            Height::Metres(h) => h,
            Height::Missing(t) => panic!("{t:?}"),
        };
        // Stone Mountain's bare granite summit is 513 m (USGS); the Chattahoochee at Atlanta's
        // Peachtree Creek about 230 m
        let summit = at(33.8053, -84.1452);
        let river = at(33.8193, -84.4530);
        println!(
            "{}: {} × {}, read in {:.2?}, kept as {:.1} MB ({:.1} MB fetched); Stone Mountain {summit:.1} m, \
             the Chattahoochee {river:.1} m",
            source.name(),
            r.width,
            r.height,
            read,
            size as f64 / 1e6,
            tif.len() as f64 / 1e6
        );
        assert!((470.0..530.0).contains(&summit), "{summit}");
        assert!((200.0..260.0).contains(&river), "{river}");
        let _ = std::fs::remove_dir_all(dir);
    }
}
