import { clockBoth } from "./localtime";
import { useEffect, useMemo, useRef, useState, MouseEvent, PointerEvent } from "react";
import { geoCircle, geoEquirectangular, geoGraticule, geoInterpolate, geoPath } from "d3-geo";
import type { Feature, LineString } from "geojson";
import { feature } from "topojson-client";
import type { Topology } from "topojson-specification";
import landTopology from "world-atlas/land-110m.json";
import { Coverage, CoverageCell, HeardStation, HearingStation, LatLon } from "./types";
import { relFill, signedDb } from "./ui";

const WIDTH = 760;
const HEIGHT = 380;
// How close the pointer must be to a heard station, in map units, to show it.
const STATION_REACH = 9;
const MIN_ZOOM = 1;
const MAX_ZOOM = 8;
const ZOOM_STEP = 1.5;
// Pointer travel, in map units, beyond which a press is a drag and not a click.
const DRAG_THRESHOLD = 3;
const projection = geoEquirectangular().fitSize([WIDTH, HEIGHT], { type: "Sphere" });
const path = geoPath(projection);
const topology = landTopology as unknown as Topology;
const land = path(feature(topology, topology.objects.land)) ?? "";
// 20 by 10 degrees is the Maidenhead field grid.
const graticule = path(geoGraticule().step([20, 10])()) ?? "";

/** Where the sun is overhead, as [longitude, latitude], for mid-month. */
function subsolarPoint(month: number, clockHour: number): [number, number] {
  const dayOfYear = (month - 1) * 30.44 + 15;
  const declination = -23.44 * Math.cos((2 * Math.PI * (dayOfYear + 10)) / 365);
  return [(12 - clockHour) * 15, declination];
}

function line(points: LatLon[]): string {
  const shape: Feature<LineString> = {
    type: "Feature",
    properties: {},
    geometry: { type: "LineString", coordinates: points.map((p) => [p.lon, p.lat]) },
  };
  return path(shape) ?? "";
}

/** The view: map content scaled by `k` about the origin, then moved by (x, y). */
type View = { k: number; x: number; y: number };
const HOME: View = { k: 1, x: 0, y: 0 };

/** Keeps the map covering the whole frame. */
function clamp(view: View): View {
  const k = Math.min(MAX_ZOOM, Math.max(MIN_ZOOM, view.k));
  return {
    k,
    x: Math.min(0, Math.max(WIDTH * (1 - k), view.x)),
    y: Math.min(0, Math.max(HEIGHT * (1 - k), view.y)),
  };
}

/** Zooms by `factor` keeping the map point under `[px, py]` (in frame units) still. */
function zoomAt(view: View, [px, py]: [number, number], factor: number): View {
  const k = Math.min(MAX_ZOOM, Math.max(MIN_ZOOM, view.k * factor));
  const ratio = k / view.k;
  return clamp({ k, x: px - (px - view.x) * ratio, y: py - (py - view.y) * ratio });
}

type Props = {
  from: LatLon | null;
  to: LatLon | null;
  longPath: boolean;
  month: number;
  clockHour: number;
  coverage: { data: Coverage; bandIndex: number } | null;
  /** Stations this receiver has decoded, drawn over the coverage. */
  heard: HeardStation[];
  /** Stations heard reporting this station's area, drawn as rings. */
  hearing?: HearingStation[];
  /** When set, a click reports the position under the pointer. */
  picking: boolean;
  onPick: (position: LatLon) => void;
};

type Hover = { x: number; y: number; cell?: CoverageCell; station?: HeardStation; hearing?: HearingStation };
type Drag = { start: [number, number]; view: View; moved: boolean };

/** The pointer in frame units, accounting for the border and any letterboxing. */
function pointAt(svg: SVGSVGElement, clientX: number, clientY: number): [number, number] | null {
  const matrix = svg.getScreenCTM();
  if (!matrix) return null;
  const { x, y } = new DOMPoint(clientX, clientY).matrixTransform(matrix.inverse());
  return [x, y];
}

/** World map with the path, day and night, predicted coverage and heard stations. */
export function WorldMap(props: Props) {
  const { from, to, longPath, month, clockHour, coverage, heard, hearing = [], picking, onPick } = props;
  const [hover, setHover] = useState<Hover | null>(null);
  const [view, setView] = useState<View>(HOME);
  const svgRef = useRef<SVGSVGElement>(null);
  const drag = useRef<Drag | null>(null);

  // The wheel zooms the map instead of scrolling the page, which needs a
  // listener React cannot register (it must not be passive).
  useEffect(() => {
    const svg = svgRef.current;
    if (!svg) return;
    const onWheel = (event: WheelEvent) => {
      event.preventDefault();
      const point = pointAt(svg, event.clientX, event.clientY);
      if (point) setView((v) => zoomAt(v, point, event.deltaY < 0 ? ZOOM_STEP : 1 / ZOOM_STEP));
    };
    svg.addEventListener("wheel", onWheel, { passive: false });
    return () => svg.removeEventListener("wheel", onWheel);
  }, []);

  const night = useMemo(() => {
    const [lon, lat] = subsolarPoint(month, clockHour);
    return path(geoCircle().center([lon + 180, -lat]).radius(90)()) ?? "";
  }, [month, clockHour]);

  const route = useMemo(() => {
    if (!from || !to) return "";
    if (!longPath) return line([from, to]);
    // The long way passes through the point opposite the short path's middle.
    const [lon, lat] = geoInterpolate([from.lon, from.lat], [to.lon, to.lat])(0.5);
    return line([from, { lat: -lat, lon: lon + 180 }, to]);
  }, [from, to, longPath]);

  /** A projected point under the current view, in frame units. */
  const place = (position: LatLon): [number, number] => {
    const [x, y] = projection([position.lon, position.lat]) ?? [0, 0];
    return [x * view.k + view.x, y * view.k + view.y];
  };

  const stations = useMemo(
    () =>
      heard.map((station) => {
        const [x, y] = projection([station.lon, station.lat]) ?? [0, 0];
        return { station, x: x * view.k + view.x, y: y * view.k + view.y };
      }),
    [heard, view],
  );

  const rings = useMemo(
    () =>
      hearing.map((station) => {
        const [x, y] = projection([station.lon, station.lat]) ?? [0, 0];
        return { station, x: x * view.k + view.x, y: y * view.k + view.y };
      }),
    [hearing, view],
  );

  function ringNear([px, py]: [number, number]): HearingStation | undefined {
    let nearest: { station: HearingStation; distance: number } | undefined;
    for (const { station, x, y } of rings) {
      const distance = Math.hypot(x - px, y - py);
      if (distance <= STATION_REACH && (!nearest || distance < nearest.distance)) {
        nearest = { station, distance };
      }
    }
    return nearest?.station;
  }

  function positionAt([px, py]: [number, number]): LatLon | null {
    const inverted = projection.invert?.([(px - view.x) / view.k, (py - view.y) / view.k]);
    return inverted ? { lon: inverted[0], lat: inverted[1] } : null;
  }

  function cellAt(position: LatLon): CoverageCell | undefined {
    if (!coverage) return undefined;
    const { latStepDeg, lonStepDeg, cells } = coverage.data;
    return cells.find(
      (c) =>
        Math.abs(c.lat - position.lat) <= latStepDeg / 2 &&
        Math.abs(c.lon - position.lon) <= lonStepDeg / 2,
    );
  }

  function stationNear([px, py]: [number, number]): HeardStation | undefined {
    let nearest: { station: HeardStation; distance: number } | undefined;
    for (const { station, x, y } of stations) {
      const distance = Math.hypot(x - px, y - py);
      if (distance <= STATION_REACH && (!nearest || distance < nearest.distance)) {
        nearest = { station, distance };
      }
    }
    return nearest?.station;
  }

  const zoomCentred = (factor: number) => setView((v) => zoomAt(v, [WIDTH / 2, HEIGHT / 2], factor));
  const atHome = view.k === 1 && view.x === 0 && view.y === 0;

  const onPointerDown = (event: PointerEvent<SVGSVGElement>) => {
    if (event.button !== 0) return;
    const point = pointAt(event.currentTarget, event.clientX, event.clientY);
    if (!point) return;
    drag.current = { start: point, view, moved: false };
    event.currentTarget.setPointerCapture(event.pointerId);
  };

  const onPointerMove = (event: PointerEvent<SVGSVGElement>) => {
    const point = pointAt(event.currentTarget, event.clientX, event.clientY);
    if (!point) return;
    const current = drag.current;
    if (current) {
      const dx = point[0] - current.start[0];
      const dy = point[1] - current.start[1];
      if (current.moved || Math.hypot(dx, dy) > DRAG_THRESHOLD) {
        current.moved = true;
        setView(clamp({ ...current.view, x: current.view.x + dx, y: current.view.y + dy }));
        setHover(null);
      }
      return;
    }
    const position = positionAt(point);
    const station = stationNear(point);
    const ring = ringNear(point);
    const cell = position ? cellAt(position) : undefined;
    const box = event.currentTarget.getBoundingClientRect();
    setHover(
      station || ring || cell
        ? { x: event.clientX - box.left, y: event.clientY - box.top, cell, station, hearing: ring }
        : null,
    );
  };

  const onPointerUp = (event: PointerEvent<SVGSVGElement>) => {
    const current = drag.current;
    drag.current = null;
    if (current && !current.moved && picking) {
      const point = pointAt(event.currentTarget, event.clientX, event.clientY);
      const position = point && positionAt(point);
      if (position) onPick(position);
    }
  };

  const marker = (position: LatLon, label: string) => {
    const [x, y] = place(position);
    return (
      <g key={label}>
        <circle className="marker" cx={x} cy={y} r={5} />
        <text className="marker-label" x={x + 8} y={y + 4}>
          {label}
        </text>
      </g>
    );
  };

  const onClick = (event: MouseEvent<SVGSVGElement>) => {
    // Picks are handled on pointer up, so a drag never counts as one.
    event.preventDefault();
  };

  return (
    <div className="map">
      <svg
        ref={svgRef}
        viewBox={`0 0 ${WIDTH} ${HEIGHT}`}
        className={picking ? "picking" : undefined}
        role="img"
        aria-label="World map showing the path, day and night, predicted coverage and heard stations"
        onClick={onClick}
        onPointerDown={onPointerDown}
        onPointerMove={onPointerMove}
        onPointerUp={onPointerUp}
        onPointerCancel={() => (drag.current = null)}
        onPointerLeave={() => setHover(null)}
      >
        <rect className="sea" width={WIDTH} height={HEIGHT} />
        <g transform={`translate(${view.x} ${view.y}) scale(${view.k})`}>
          <path className="land" d={land} />
          {coverage?.data.cells.map((cell) => {
            const { latStepDeg, lonStepDeg } = coverage.data;
            const [x0, y0] = projection([cell.lon - lonStepDeg / 2, cell.lat + latStepDeg / 2]) ?? [0, 0];
            const [x1, y1] = projection([cell.lon + lonStepDeg / 2, cell.lat - latStepDeg / 2]) ?? [0, 0];
            return (
              <rect
                key={`${cell.lat},${cell.lon}`}
                x={x0}
                y={y0}
                width={x1 - x0}
                height={y1 - y0}
                className="coverage-cell"
                style={relFill(cell.reliability[coverage.bandIndex])}
              />
            );
          })}
          <path className="graticule" d={graticule} vectorEffect="non-scaling-stroke" />
          {coverage && <path className="coast" d={land} vectorEffect="non-scaling-stroke" />}
          <path className="night" d={night} />
          {route && <path className="route-halo" d={route} vectorEffect="non-scaling-stroke" />}
          {route && <path className="route" d={route} vectorEffect="non-scaling-stroke" />}
        </g>
        {rings.map(({ station, x, y }) => (
          <g key={`ring ${station.band} ${station.callsign}`}>
            <circle className="hearing-halo" cx={x} cy={y} r={6.5} />
            <circle className="hearing-station" cx={x} cy={y} r={6.5} />
          </g>
        ))}
        {stations.map(({ station, x, y }) => (
          <circle key={`${station.band} ${station.callsign}`} className="heard-station" cx={x} cy={y} r={4} />
        ))}
        {from && marker(from, "From")}
        {to && marker(to, "To")}
      </svg>
      <div className="map-controls" role="group" aria-label="Zoom">
        <button type="button" onClick={() => zoomCentred(ZOOM_STEP)} disabled={view.k >= MAX_ZOOM} aria-label="Zoom in">
          +
        </button>
        <button type="button" onClick={() => zoomCentred(1 / ZOOM_STEP)} disabled={view.k <= MIN_ZOOM} aria-label="Zoom out">
          −
        </button>
        <button type="button" onClick={() => setView(HOME)} disabled={atHome} aria-label="Whole world">
          Reset
        </button>
      </div>
      {hover && (
        <div className="tooltip" style={{ left: hover.x + 14, top: hover.y + 14 }}>
          {hover.station && (
            <>
              <div>
                <strong>{hover.station.callsign}</strong> {hover.station.grid}, {hover.station.band}
              </div>
              <div>
                <strong>{hover.station.bestSnrDb} dB</strong> best SNR, {hover.station.decodes}{" "}
                {hover.station.decodes === 1 ? "decode" : "decodes"}
              </div>
              <div className="tooltip-title">
                {hover.station.distanceKm !== null && `${hover.station.distanceKm.toFixed(0)} km, `}
                last heard {clockBoth(hover.station.lastHeardUtc)}
              </div>
            </>
          )}
          {hover.hearing && (
            <div className={hover.station ? "tooltip-more" : undefined}>
              <div>
                <strong>{hover.hearing.callsign}</strong> {hover.hearing.grid}, {hover.hearing.band}, hears your area
              </div>
              <div>
                {hover.hearing.reported
                  .map((r) => (r.distanceKm === 0 ? `you ${signedDb(r.reportDb)}` : `${r.callsign} ${signedDb(r.reportDb)}`))
                  .join(", ")}
              </div>
              <div className="tooltip-title">last report {clockBoth(hover.hearing.lastUtc)}</div>
            </div>
          )}
          {hover.cell && coverage && (
            <>
              <div className="tooltip-title">
                Predicted at {Math.abs(hover.cell.lat)}°{hover.cell.lat >= 0 ? "N" : "S"}{" "}
                {Math.abs(hover.cell.lon)}°{hover.cell.lon >= 0 ? "E" : "W"},{" "}
                {hover.cell.distanceKm.toFixed(0)} km
              </div>
              <div>
                <strong>{(hover.cell.reliability[coverage.bandIndex] * 100).toFixed(0)}%</strong>{" "}
                reliability, <strong>{hover.cell.snrDb[coverage.bandIndex].toFixed(0)} dB-Hz</strong> SNR
              </div>
            </>
          )}
        </div>
      )}
    </div>
  );
}
