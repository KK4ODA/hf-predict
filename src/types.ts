// Mirrors the serialised Rust types in src-tauri/src.

export type StationProfile = {
  name: string;
  powerWatts: number;
  antenna: string;
  noiseDb: number;
  minAngleDeg: number;
};

export type Choice<T> = { value: T; label: string };
export type Mode = "ssb" | "cw" | "ft8";
export type Band = { name: string; mhz: number };

export type Options = {
  presets: StationProfile[];
  antennas: Choice<string>[];
  noiseLevels: Choice<number>[];
  modes: Choice<Mode>[];
  bands: Band[];
};

export type SavedLocation = { name: string; position: string };
export type UserData = { locations: SavedLocation[]; stations: StationProfile[] };

export type PathRequest = {
  txPosition: string;
  rxPosition: string;
  year: number;
  month: number;
  ssn: number | null;
  txStation: StationProfile;
  rxStation: StationProfile;
  mode: Mode;
  requiredReliabilityPct: number;
  longPath: boolean;
};

export type FrequencyPrediction = {
  freqMhz: number;
  mode: string;
  takeoffAngleDeg: number;
  delayMs: number;
  virtualHeightKm: number;
  mufDay: number;
  lossDb: number;
  fieldStrengthDbu: number;
  signalDbw: number;
  noiseDbw: number;
  snrDb: number;
  requiredPowerGainDb: number;
  reliability: number;
  multipathProbability: number;
  serviceProbability: number;
  signalLowerDecileDb: number;
  signalUpperDecileDb: number;
  snrLowerDecileDb: number;
  snrUpperDecileDb: number;
  txGainDbi: number;
  rxGainDbi: number;
  snrAtRequiredReliabilityDb: number;
};

export type HourPrediction = {
  utcHour: number;
  mufMhz: number;
  atMuf: FrequencyPrediction;
  frequencies: FrequencyPrediction[];
};

export type Prediction = {
  engine: string;
  distanceKm: number;
  azimuthTxDeg: number;
  azimuthRxDeg: number;
  hours: HourPrediction[];
};

export type SsnUsed = {
  value: number;
  kind: "observed" | "predicted" | "manual";
  source: string;
  tableGenerated: string;
};

export type PathPrediction = {
  tx: { lat: number; lon: number };
  rx: { lat: number; lon: number };
  txLocator: string;
  rxLocator: string;
  ssn: SsnUsed;
  requiredSnrDbHz: number;
  bands: Band[];
  engine: string;
  run: { prediction: Prediction; input: string; output: string };
};
