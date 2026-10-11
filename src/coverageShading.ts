// Coverage shading: the reliability steps drawn as filled areas between the
// predicted points, so the map shows where a band reaches rather than the
// blocks of the grid it was computed on.

import { contours } from "d3-contour";
import { Coverage, CoverageCell } from "./types";
import { REL_EDGES } from "./ui";

/** Map units per degree, and where longitude -180, latitude 90 falls. */
export type Frame = { left: number; top: number; xPerDeg: number; yPerDeg: number };

const rowOf = (data: Coverage, lat: number) => Math.floor((lat + 90) / data.latStepDeg);
const columnOf = (data: Coverage, lon: number) => Math.floor((lon + 180) / data.lonStepDeg);

/** Looks up the predicted cell that holds a position. */
export function cellFinder(data: Coverage): (lat: number, lon: number) => CoverageCell | undefined {
  const cells = new Map(data.cells.map((c) => [`${rowOf(data, c.lat)},${columnOf(data, c.lon)}`, c]));
  return (lat, lon) => cells.get(`${rowOf(data, lat)},${columnOf(data, lon)}`);
}

/**
 * Reliability on the grid, north row first, with one extra column at each
 * side (wrapped around the date line) and one extra row at each pole, so the
 * shading reaches the edges of the map. Cells left out for being too near
 * the transmitter take the mean of their neighbours.
 */
function gridValues(data: Coverage, bandIndex: number): { values: number[]; width: number; height: number } {
  const rows = Math.round(180 / data.latStepDeg);
  const columns = Math.round(360 / data.lonStepDeg);
  const grid: (number | undefined)[][] = Array.from({ length: rows }, () => Array<number | undefined>(columns));
  for (const cell of data.cells) {
    grid[rows - 1 - rowOf(data, cell.lat)][columnOf(data, cell.lon)] = cell.reliability[bandIndex];
  }
  for (let pass = 0; pass < 4; pass++) {
    let missing = 0;
    for (let r = 0; r < rows; r++) {
      for (let c = 0; c < columns; c++) {
        if (grid[r][c] !== undefined) continue;
        let sum = 0;
        let count = 0;
        for (let dr = -1; dr <= 1; dr++) {
          for (let dc = -1; dc <= 1; dc++) {
            const value = grid[r + dr]?.[(c + dc + columns) % columns];
            if (value !== undefined) {
              sum += value;
              count++;
            }
          }
        }
        if (count > 0) grid[r][c] = sum / count;
        else missing++;
      }
    }
    if (missing === 0) break;
  }

  const width = columns + 2;
  const height = rows + 2;
  const values: number[] = [];
  for (let y = 0; y < height; y++) {
    const r = Math.min(rows - 1, Math.max(0, y - 1));
    for (let x = 0; x < width; x++) {
      values.push(grid[r][(x - 1 + columns) % columns] ?? 0);
    }
  }
  return { values, width, height };
}

/**
 * One outline per reliability step, the area at or above its lower edge, as
 * SVG path data in map units. Painted in order, each covers the one before,
 * which leaves the steps as bands.
 */
export function shadingPaths(data: Coverage, bandIndex: number, frame: Frame): string[] {
  const { values, width, height } = gridValues(data, bandIndex);
  // A value at grid index (x, y) sits at contour coordinates (x + 0.5, y + 0.5);
  // index 1 is the first real column and row.
  const x = (cx: number) => (frame.left + (cx - 1) * data.lonStepDeg * frame.xPerDeg).toFixed(2);
  const y = (cy: number) => (frame.top + (cy - 1) * data.latStepDeg * frame.yPerDeg).toFixed(2);
  return contours()
    .size([width, height])
    .thresholds([0, ...REL_EDGES])(values)
    .map((shape) =>
      shape.coordinates
        .flat()
        .map((ring) => `M${ring.map(([cx, cy]) => `${x(cx)},${y(cy)}`).join("L")}Z`)
        .join(""),
    );
}
