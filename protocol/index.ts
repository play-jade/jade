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

/** The published dataset release that answered an archive request. */
export interface ArchiveRelease {
  dataset_id: string;
  release_id: string;
  is_synthetic_fixture: boolean;
}

export interface SourceAttribution {
  label: string;
  url: string | null;
  locator: string | null;
}

export interface ArchiveAccessPolicy {
  ordinary_search_and_replay: string;
  individual_game_export: string;
  collection_bulk_export: string;
}

export interface ArchiveCollectionSummary {
  release: ArchiveRelease;
  title: string;
  scope: string;
  provenance_banner: string;
  source: SourceAttribution;
  access_policy: ArchiveAccessPolicy;
  game_count: number;
}

export interface ArchivePlayerAlias {
  text: string;
  script: string;
  source: string;
  verification_state: string;
}

export interface ArchivePlayer {
  player_id: string;
  primary_display_name: string;
  native_display_name: string;
  disambiguation_note: string;
  aliases: ArchivePlayerAlias[];
}

export interface ArchivePlayerSearch {
  release: ArchiveRelease;
  provenance_banner: string;
  players: ArchivePlayer[];
}

export interface ArchiveGameSummary {
  game_id: string;
  red_player_id: string;
  red_display_name: string;
  black_player_id: string;
  black_display_name: string;
  event_name: string;
  round: string | null;
  date_precision: string;
  date_display: string;
  recorded_result: string;
  replay_validity: string;
}

export interface ArchiveGameQuery {
  player_id?: string;
  text?: string;
  offset?: number;
  limit?: number;
}

/** What a game-directory query found; position analysis states are separate. */
export type ArchiveDirectoryStatus = "ok" | "no_games_found";

export interface ArchiveGameSummaryPage {
  release: ArchiveRelease;
  status: ArchiveDirectoryStatus;
  total: number;
  offset: number;
  limit: number;
  games: ArchiveGameSummary[];
}

export interface ArchivePlayerProfile {
  release: ArchiveRelease;
  player: ArchivePlayer;
  games: ArchiveGameSummaryPage;
}

export interface ArchiveGameRecord {
  game_id: string;
  dataset_id: string;
  is_synthetic_fixture: boolean;
  event_name: string;
  date_precision: string;
  date_display: string;
  participant_kind: string;
  game_context: string;
  red_player_id: string;
  black_player_id: string;
  initial_fen: string;
  moves_uci: string[];
  moves_chinese: string[];
  recorded_result: string;
  recorded_result_reason: string;
  original_rules_provenance: string;
  replay_validity: string;
}

export interface ArchiveGameDetail {
  release: ArchiveRelease;
  summary: ArchiveGameSummary;
  source: SourceAttribution;
  record: ArchiveGameRecord;
}

export interface ArchivePositionContinuation {
  move_uci: string;
  move_chinese: string;
  deduplicated_game_count: number;
  total_occurrence_count: number;
  known_result_count: number;
  red_wins: number;
  draws: number;
  black_wins: number;
  contributing_game_ids: string[];
}

export interface ArchivePositionResponse {
  release: ArchiveRelease;
  dataset_id: string;
  is_synthetic_fixture: boolean;
  position_key_sha256: string;
  status: PositionAvailabilityState;
  evaluation_status: PositionAvailabilityState;
  sample_size_games: number;
  continuations: ArchivePositionContinuation[];
}

/**
 * Error codes at the archive boundary. `archive_provider_unavailable` means a configured hosted
 * archive did not answer; a client shows it with a retry and never substitutes sample records.
 */
export type ArchiveErrorCode =
  | "archive_not_found"
  | "archive_invalid_request"
  | "archive_provider_unavailable";

export interface ArchiveError {
  code: ArchiveErrorCode;
  message: string;
}
