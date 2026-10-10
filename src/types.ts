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
export type UserData = { locations: SavedLocation[]; stations: StationProfile[]; logFiles: LogFile[] };

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
  distanceKm: number;
  txBearingDeg: number;
  rxBearingDeg: number;
  ssn: SsnUsed;
  requiredSnrDbHz: number;
  bands: Band[];
  engine: string;
  run: { prediction: Prediction; input: string; output: string };
};

export type PowerCase = {
  powerWatts: number;
  /** [hour][band], in the main prediction's order. */
  reliability: number[][];
  snrDb: number[][];
};

export type FrequencyWindow = {
  utcHour: number;
  mufMhz: number;
  fotMhz: number | null;
  lufMhz: number | null;
};

export type PathDetail = {
  prediction: PathPrediction;
  power: PowerCase[];
  window: FrequencyWindow[];
  /** Reliability for FT8 with the same stations, [hour][band]. */
  ft8Reliability: number[][];
};

export type BandComparison = {
  band: string;
  modeReliability: number;
  ft8Reliability: number;
  predicted: "good" | "marginal" | "poor";
  periods: number;
  bandDecodes: number;
  bandCallsigns: number;
  evidenceStations: number;
  evidenceBestSnrDb: number | null;
  evidenceExamples: string[];
  observed: "notSampled" | "none" | "limited" | "moderate" | "strong";
  verdict: { label: string; detail: string; priority: number };
};

export type PathOverview = { short: PathDetail; long: PathDetail };

export type LatLon = { lat: number; lon: number };

export type CoverageCell = {
  lat: number;
  lon: number;
  distanceKm: number;
  /** One value per band, in `Coverage.bands` order. */
  reliability: number[];
  snrDb: number[];
};

export type Coverage = {
  tx: LatLon;
  utcHour: number;
  latStepDeg: number;
  lonStepDeg: number;
  ssn: SsnUsed;
  requiredSnrDbHz: number;
  bands: Band[];
  cells: CoverageCell[];
};

export type Transport = "internet" | "winlink" | "pasted" | "file";

export type Wwv = {
  kind: "wwv";
  solarFlux: number | null;
  aIndex: number | null;
  kIndex: number | null;
  kTime: string | null;
  past24h: string;
  next24h: string;
};

export type Sgas = {
  kind: "sgas";
  dataDate: string | null;
  solarFlux: number | null;
  sunspotNumber: number | null;
  aFredericksburg: number | null;
  aPlanetary: number | null;
  xrayBackground: string | null;
  planetaryK: (number | null)[];
  energeticEvents: string[];
  protonEvents: string | null;
  geomagneticSummary: string | null;
};

export type ThreeDay = {
  kind: "threeDay";
  days: string[];
  kp: { period: string; values: (number | null)[] }[];
  observedMaxKp: number | null;
  expectedMaxKp: number | null;
  radiationStormPct: (number | null)[];
  blackoutR1R2Pct: (number | null)[];
  blackoutR3Pct: (number | null)[];
  geomagneticRationale: string | null;
  blackoutRationale: string | null;
};

export type Outlook27 = {
  kind: "outlook27";
  days: { date: string; solarFlux: number | null; aIndex: number | null; maxKp: number | null }[];
};

export type Product = Wwv | Sgas | ThreeDay | Outlook27;

export type StoredProduct = {
  product: Product;
  /** Seconds since the Unix epoch. */
  issued: number;
  received: number;
  transport: Transport;
  text: string;
};

export type ProductStatus = {
  kind: string;
  title: string;
  sourceUrl: string;
  winlinkId: string;
  stored: StoredProduct | null;
  ageSeconds: number | null;
  staleAfterSeconds: number;
  stale: boolean;
};

export type SsnTableStatus = {
  source: string;
  generated: string;
  generatedUnix: number;
  downloaded: boolean;
  lastObservedMonth: string;
  lastPredictedMonth: string;
  ageSeconds: number;
  stale: boolean;
};

export type Conditions = {
  now: number;
  products: ProductStatus[];
  ssnTable: SsnTableStatus;
  storm: string | null;
};

export type FetchResult = { title: string; ok: boolean; detail: string };

export type Imported = {
  title: string;
  outcome: "stored" | "alreadyHave" | "olderThanStored" | "notUnderstood";
  detail: string | null;
};

export type ConditionsUpdate<T> = { results: T[]; conditions: Conditions };

export type ListenerConfig = { enabled: boolean; address: string; port: number };

export type DecoderStatus = {
  id: string;
  version: string | null;
  dialHz: number | null;
  band: string | null;
  mode: string | null;
  deCall: string | null;
  deGrid: string | null;
  transmitting: boolean;
  secondsSinceHeard: number;
};

export type ClockCheck = {
  medianDtS: number | null;
  samples: number;
  level: "unknown" | "ok" | "warn" | "alarm";
};

export type TrackerStatus = {
  decoders: DecoderStatus[];
  clock: ClockCheck;
  stored: number;
  skipped: number;
};

export type ListenerStatus = {
  config: ListenerConfig;
  state: "off" | "listening" | "failed";
  detail: string;
  datagrams: number;
  notUnderstood: number;
  lastError: string | null;
  tracker: TrackerStatus | null;
};

export type Observation = {
  /** Start of the transmit period, seconds since the Unix epoch. */
  timeUtc: number;
  dialHz: number;
  band: string;
  dfHz: number;
  snrDb: number;
  dtS: number;
  mode: string;
  message: string;
  kind: string;
  sender: string | null;
  addressee: string | null;
  grid: string | null;
  gridSource: "message" | "remembered" | null;
  distanceKm: number | null;
  bearingDeg: number | null;
  rxGrid: string | null;
  origin: string;
  provider: string;
  lowConfidence: boolean;
  settling: boolean;
};

export type BandActivity = {
  band: string;
  dialHz: number;
  listenedSeconds: number;
  /** Transmit periods listened to. */
  periods: number;
  decodes: number;
  decodesPerPeriod: number | null;
  uniqueCallsigns: number;
  uniqueGrids: number;
  medianSnrDb: number | null;
  p90SnrDb: number | null;
  locatedStations: number;
  medianDistanceKm: number | null;
  maxDistanceKm: number | null;
  longDistanceStations: number;
  /** Located stations per 45-degree compass sector, north first, clockwise. */
  sectors: number[];
  previousUniqueCallsigns: number;
  previousPeriods: number;
};

export type HeardStation = {
  callsign: string;
  grid: string;
  lat: number;
  lon: number;
  band: string;
  decodes: number;
  bestSnrDb: number;
  lastHeardUtc: number;
  distanceKm: number | null;
  bearingDeg: number | null;
};

export type ImportSummary = {
  stored: number;
  alreadyStored: number;
  transmissions: number;
  notUnderstood: number;
};

export type CalibrationBin = { decodes: number; opportunities: number; heard: number };

/** Hearing tallied against predicted reliability, in equal bins from 0 to 100%. */
export type CalibrationReport = {
  receiver: string;
  circuits: number;
  circuitsComputed: number;
  decodesUsed: number;
  decodesSkipped: number;
  overall: CalibrationBin[];
  byBand: Record<string, CalibrationBin[]>;
};

/** A WSJT-X ALL.TXT log to read, and where its receiver was. */
export type LogFile = { path: string; rxPosition: string | null };

export type FoundLog = { path: string; program: string; sizeBytes: number; modifiedUtc: number | null };

export type LogCheck = {
  path: string;
  ok: boolean;
  detail: string;
  checkedUtc: number;
  sizeBytes: number;
  newBytes: number;
  summary: ImportSummary | null;
};

export type PlanTier = "long" | "standard" | "probe";

export type BandRank = {
  band: string;
  dialHz: number;
  prediction: number;
  observed: "strong" | "moderate" | "limited" | "none" | "notSampled";
  minutesSinceListened: number | null;
  priority: number;
  tier: PlanTier | null;
  reason: string;
};

export type PlanItem = { band: string; dialHz: number; startS: number; dwellS: number; tier: PlanTier };

/** Which bands to listen on, in what order and for how long. */
export type Plan = {
  minutes: number;
  bands: BandRank[];
  hopBands: string[];
  cycleS: number;
  items: PlanItem[];
};

export type RadioConfig = { enabled: boolean; host: string; port: number; pollSeconds: number };

export type RadioState = {
  freqHz: number;
  band: string;
  mode: string;
  passbandHz: number | null;
  ptt: boolean;
  split: boolean | null;
  vfo: string | null;
  txVfo: string | null;
};

export type RadioStatus = {
  config: RadioConfig;
  state: "off" | "connecting" | "connected" | "failed";
  detail: string;
  radio: RadioState | null;
  readUtc: number | null;
  reads: number;
  errors: number;
  lastError: string | null;
};
