// Builds src-tauri/data/smoothed-ssn.json from NOAA SWPC's solar-cycle tables.
//
//   node tools/update-ssn.mjs
//
// The app bundles this table so predictions work with no network. Observed
// smoothed values lag about six months; predicted values continue from there.

import { writeFile } from "node:fs/promises";

const BASE = "https://services.swpc.noaa.gov/json/solar-cycle";
const FIRST_MONTH = "1990-01";

async function get(name) {
  const response = await fetch(`${BASE}/${name}`);
  if (!response.ok) throw new Error(`${name}: HTTP ${response.status}`);
  return response.json();
}

function nextMonth(tag) {
  const [year, month] = tag.split("-").map(Number);
  return month === 12
    ? `${year + 1}-01`
    : `${year}-${String(month + 1).padStart(2, "0")}`;
}

function assertContiguous(rows, what) {
  for (let i = 1; i < rows.length; i++) {
    if (rows[i]["time-tag"] !== nextMonth(rows[i - 1]["time-tag"])) {
      throw new Error(`${what}: gap after ${rows[i - 1]["time-tag"]}`);
    }
  }
}

const [observedRows, predictedRows] = await Promise.all([
  get("observed-solar-cycle-indices.json"),
  get("predicted-solar-cycle.json"),
]);

const observed = observedRows.filter(
  (row) => row["time-tag"] >= FIRST_MONTH && row.smoothed_ssn >= 0,
);
const lastObserved = observed.at(-1)["time-tag"];
const predicted = predictedRows.filter((row) => row["time-tag"] > lastObserved);

assertContiguous(observed, "observed");
assertContiguous(predicted, "predicted");
if (predicted[0]["time-tag"] !== nextMonth(lastObserved)) {
  throw new Error(`predicted table does not continue from ${lastObserved}`);
}

const table = {
  source:
    "NOAA SWPC solar-cycle tables (services.swpc.noaa.gov), monthly smoothed sunspot number",
  generated: new Date().toISOString().slice(0, 10),
  observed: {
    start: observed[0]["time-tag"],
    values: observed.map((row) => row.smoothed_ssn),
  },
  predicted: {
    start: predicted[0]["time-tag"],
    values: predicted.map((row) => row.predicted_ssn),
  },
};

const target = new URL("../src-tauri/data/smoothed-ssn.json", import.meta.url);
await writeFile(target, JSON.stringify(table) + "\n");
console.log(
  `observed ${table.observed.start}..${lastObserved}, ` +
    `predicted ${table.predicted.start}..${predicted.at(-1)["time-tag"]}`,
);
