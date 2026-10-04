import { useMemo, useState, MouseEvent } from "react";
import { geoCircle, geoEquirectangular, geoGraticule, geoInterpolate, geoPath } from "d3-geo";
import type { Feature, LineString } from "geojson";
import { feature } from "topojson-client";
import type { Topology } from "topojson-specification";
import landTopology from "world-atlas/land-110m.json";
import { Coverage, CoverageCell, LatLon } from "./types";
import { shade } from "./tiers";

const WIDTH = 760;
const HEIGHT = 380;
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
  /** When set, a click reports the position under the pointer. */
  picking: boolean;
  onPick: (position: LatLon) => void;
};

/** World map with the path, day and night, and optional coverage cells. */
export function WorldMap({ from, to, longPath, month, clockHour, coverage, picking, onPick }: Props) {
  const [hover, setHover] = useState<{ cell: CoverageCell; x: number; y: number } | null>(null);

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

  function positionAt(event: MouseEvent<SVGSVGElement>): LatLon | null {
    // The screen matrix accounts for the border and any letterboxing.
    const matrix = event.currentTarget.getScreenCTM();
    if (!matrix) return null;
    const { x, y } = new DOMPoint(event.clientX, event.clientY).matrixTransform(matrix.inverse());
    const point = projection.invert?.([x, y]);
    return point ? { lon: point[0], lat: point[1] } : null;
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
        aria-label="World map showing the path, day and night, and coverage"
        onClick={(event) => {
          const position = picking && positionAt(event);
          if (position) onPick(position);
        }}
        onPointerMove={(event) => {
          const position = positionAt(event);
          const cell = position && cellAt(position);
          const box = event.currentTarget.getBoundingClientRect();
          setHover(
            cell
              ? { cell, x: event.clientX - box.left, y: event.clientY - box.top }
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
        {from && marker(from, "From")}
        {to && marker(to, "To")}
      </svg>
      {hover && coverage && (
        <div className="tooltip" style={{ left: hover.x + 14, top: hover.y + 14 }}>
          <div className="tooltip-title">
            {Math.abs(hover.cell.lat)}°{hover.cell.lat >= 0 ? "N" : "S"}{" "}
            {Math.abs(hover.cell.lon)}°{hover.cell.lon >= 0 ? "E" : "W"} ·{" "}
            {hover.cell.distanceKm.toFixed(0)} km
          </div>
          <div>
            <strong>{(hover.cell.reliability[coverage.bandIndex] * 100).toFixed(0)}%</strong>{" "}
            reliability
          </div>
          <div>
            <strong>{hover.cell.snrDb[coverage.bandIndex].toFixed(0)} dB-Hz</strong> SNR
          </div>
        </div>
      )}
    </div>
  );
}
