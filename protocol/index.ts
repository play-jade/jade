export const PROTOCOL_VERSION = 1 as const;
export const ADOPTED_RULES_PROFILE = "program-2023.11-v1" as const;

export type Side = "red" | "black";
export type Role =
  | "general"
  | "advisor"
  | "elephant"
  | "chariot"
  | "horse"
  | "cannon"
  | "soldier";

export type Visibility = "private" | "unlisted" | "public";

export type LocaleCode =
  | "en-US"
  | "en-GB"
  | "zh-Hans-CN"
  | "zh-Hant-TW"
  | "zh-Hant-HK";

export type MaterialId = "pale-jade" | "boxwood" | "ivory-stone";
export type GlyphSetId = "simplified-prototype-v1" | "traditional-prototype-v1";
export type NotationStyle = "chinese" | "wxf" | "iccs" | "uci";

export interface PresentationPreferences {
  locale: LocaleCode;
  glyphSet: GlyphSetId;
  material: MaterialId;
  notation: NotationStyle;
  orientation: Side;
  darkSurface: boolean;
  soundEnabled: boolean;
  soundVolume: number;
  reducedMotion: boolean;
}

export interface MoveCommand {
  type: "move";
  protocol: 1;
  match_id: string;
  command_id: string;
  expected_seq: number;
  move: {
    from: [number, number];
    to: [number, number];
  };
}

export interface MatchActionCommand {
  type: "offer_draw" | "accept_draw" | "decline_draw" | "resign" | "request_takeback" | "accept_takeback";
  protocol: 1;
  match_id: string;
  command_id: string;
  expected_seq: number;
}

export interface SearchAdapterRequest {
  search_id: string;
  state_id: string;
  engine_id: string;
  binary_hash: string;
  network_hash: string | null;
  rules_profile: string;
  initial_fen: string;
  moves_uci: string[];
  budget: {
    depth?: number;
    movetime_ms?: number;
    nodes?: number;
  };
  multipv: number;
  config_hash: string;
}

export interface NormalizedSearchInfo {
  search_id: string;
  state_id: string;
  multipv: number;
  depth: number;
  seldepth?: number;
  score_kind: "cp" | "mate";
  score_red_perspective: number;
  wdl?: [number, number, number];
  nodes: number;
  nps: number;
  elapsed_ms: number;
  pv_uci: string[];
  raw_line: string;
}

export interface ArchiveContinuationSummary {
  move_uci: string;
  move_chinese: string;
  deduplicated_games: number;
  known_result_games: number;
  red_wins: number;
  draws: number;
  black_wins: number;
}

export type PositionAvailabilityState =
  | "ok"
  | "no_games_found"
  | "not_yet_parsed"
  | "excluded_from_dataset"
  | "not_analysed";
