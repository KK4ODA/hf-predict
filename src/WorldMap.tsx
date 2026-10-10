import { clockBoth } from "./localtime";
import { useMemo, useState, MouseEvent } from "react";
import { geoCircle, geoEquirectangular, geoGraticule, geoInterpolate, geoPath } from "d3-geo";
import type { Feature, LineString } from "geojson";
import { feature } from "topojson-client";
import type { Topology } from "topojson-specification";
import landTopology from "world-atlas/land-110m.json";
import { Coverage, CoverageCell, HeardStation, LatLon } from "./types";
import { shade } from "./tiers";

const WIDTH = 760;
const HEIGHT = 380;
// How close the pointer must be to a heard station, in map units, to show it.
const STATION_REACH = 9;
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

type Props = {
  from: LatLon | null;
  to: LatLon | null;
  longPath: boolean;
  month: number;
  clockHour: number;
  coverage: { data: Coverage; bandIndex: number } | null;
  /** Stations this receiver has decoded, drawn over the coverage. */
  heard: HeardStation[];
  /** When set, a click reports the position under the pointer. */
  picking: boolean;
  onPick: (position: LatLon) => void;
};

type Hover = { x: number; y: number; cell?: CoverageCell; station?: HeardStation };

/** World map with the path, day and night, predicted coverage and heard stations. */
export function WorldMap(props: Props) {
  const { from, to, longPath, month, clockHour, coverage, heard, picking, onPick } = props;
  const [hover, setHover] = useState<Hover | null>(null);

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

  const stations = useMemo(
    () =>
      heard.map((station) => {
        const [x, y] = projection([station.lon, station.lat]) ?? [0, 0];
        return { station, x, y };
      }),
    [heard],
  );

  /** The pointer in map units, accounting for the border and any letterboxing. */
  function pointAt(event: MouseEvent<SVGSVGElement>): [number, number] | null {
    const matrix = event.currentTarget.getScreenCTM();
    if (!matrix) return null;
    const { x, y } = new DOMPoint(event.clientX, event.clientY).matrixTransform(matrix.inverse());
    return [x, y];
  }

  function positionAt(point: [number, number]): LatLon | null {
    const inverted = projection.invert?.(point);
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

  const marker = (position: LatLon, label: string) => {
    const [x, y] = projection([position.lon, position.lat]) ?? [0, 0];
    return (
      <g key={label}>
        <circle className="marker" cx={x} cy={y} r={5} />
        <text className="marker-label" x={x + 8} y={y + 4}>
          {label}
        </text>
      </g>
    );
  };

  return (
    <div className="map">
      <svg
        viewBox={`0 0 ${WIDTH} ${HEIGHT}`}
        className={picking ? "picking" : undefined}
        role="img"
        aria-label="World map showing the path, day and night, predicted coverage and heard stations"
        onClick={(event) => {
          const point = picking && pointAt(event);
          const position = point && positionAt(point);
          if (position) onPick(position);
        }}
        onPointerMove={(event) => {
          const point = pointAt(event);
          const position = point && positionAt(point);
          const station = point ? stationNear(point) : undefined;
          const cell = position ? cellAt(position) : undefined;
          const box = event.currentTarget.getBoundingClientRect();
          setHover(
            station || cell
              ? { x: event.clientX - box.left, y: event.clientY - box.top, cell, station }
              : null,
          );
        }}
        onPointerLeave={() => setHover(null)}
      >
        <rect className="sea" width={WIDTH} height={HEIGHT} />
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
              fill={shade(cell.reliability[coverage.bandIndex])}
            />
          );
        })}
        <path className="graticule" d={graticule} />
        <path className="night" d={night} />
        {route && <path className="route" d={route} />}
        {stations.map(({ station, x, y }) => (
          <circle key={`${station.band} ${station.callsign}`} className="heard-station" cx={x} cy={y} r={3.5} />
        ))}
        {from && marker(from, "From")}
        {to && marker(to, "To")}
      </svg>
      {hover && (
        <div className="tooltip" style={{ left: hover.x + 14, top: hover.y + 14 }}>
          {hover.station && (
            <>
              <div>
                <strong>{hover.station.callsign}</strong> {hover.station.grid} · {hover.station.band}
              </div>
              <div>
                <strong>{hover.station.bestSnrDb} dB</strong> best SNR, {hover.station.decodes}{" "}
                {hover.station.decodes === 1 ? "decode" : "decodes"}
              </div>
              <div className="tooltip-title">
                {hover.station.distanceKm !== null && `${hover.station.distanceKm.toFixed(0)} km · `}
                last heard {clockBoth(hover.station.lastHeardUtc)}
              </div>
            </>
          )}
          {hover.cell && coverage && (
            <>
              <div className="tooltip-title">
                Predicted at {Math.abs(hover.cell.lat)}°{hover.cell.lat >= 0 ? "N" : "S"}{" "}
                {Math.abs(hover.cell.lon)}°{hover.cell.lon >= 0 ? "E" : "W"} ·{" "}
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
