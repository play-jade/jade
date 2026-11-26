use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use xiangqi_core::{
    ADOPTED_RULES_PROFILE, GameState, Move, NotationDialect, PositionKey, format_chinese_move,
};

const EMBEDDED_SAMPLE_COLLECTION: &str =
    include_str!("../../../data/fixtures/archive/sample_collection.json");

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Visibility {
    #[default]
    Private,
    Unlisted,
    Public,
}

/// Dual-participant privacy and research-consent policy for personal matches (Section 3 & 16).
/// Personal games start `Private`; sharing and each research/training/dataset use require both
/// participants' explicit agreement independently.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MatchPrivacyConsent {
    pub red_visibility: Visibility,
    pub black_visibility: Visibility,
    pub red_allow_internal_training: bool,
    pub black_allow_internal_training: bool,
    pub red_allow_published_research: bool,
    pub black_allow_published_research: bool,
    pub red_allow_dataset_release: bool,
    pub black_allow_dataset_release: bool,
}

impl Default for MatchPrivacyConsent {
    fn default() -> Self {
        Self {
            red_visibility: Visibility::Private,
            black_visibility: Visibility::Private,
            red_allow_internal_training: false,
            black_allow_internal_training: false,
            red_allow_published_research: false,
            black_allow_published_research: false,
            red_allow_dataset_release: false,
            black_allow_dataset_release: false,
        }
    }
}

impl MatchPrivacyConsent {
    /// Effective match visibility is the more restrictive of the two participants' choices.
    pub const fn effective_visibility(&self) -> Visibility {
        use Visibility::*;
        match (self.red_visibility, self.black_visibility) {
            (Public, Public) => Public,
            (Public | Unlisted, Public | Unlisted) => Unlisted,
            _ => Private,
        }
    }

    pub const fn eligible_for_internal_training(&self) -> bool {
        self.red_allow_internal_training && self.black_allow_internal_training
    }

    pub const fn eligible_for_published_research(&self) -> bool {
        self.red_allow_published_research && self.black_allow_published_research
    }

    pub const fn eligible_for_dataset_release(&self) -> bool {
        self.red_allow_dataset_release && self.black_allow_dataset_release
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AliasAssertion {
    pub text: String,
    pub script: String,
    pub source: String,
    pub verification_state: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlayerSummary {
    pub player_id: String,
    pub primary_display_name: String,
    pub native_display_name: String,
    pub disambiguation_note: String,
    pub aliases: Vec<AliasAssertion>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArchiveGameRecord {
    pub game_id: String,
    pub dataset_id: String,
    pub is_synthetic_fixture: bool,
    pub event_name: String,
    pub date_precision: String,
    pub date_display: String,
    pub participant_kind: String,
    pub game_context: String,
    pub red_player_id: String,
    pub black_player_id: String,
    pub initial_fen: String,
    pub moves_uci: Vec<String>,
    pub moves_chinese: Vec<String>,
    pub recorded_result: String, // "red_win" | "black_win" | "draw" | "unknown" | "ongoing"
    pub recorded_result_reason: String,
    pub original_rules_provenance: String,
    pub replay_validity: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccessPolicySummary {
    pub ordinary_search_and_replay: String,
    pub individual_game_export: String,
    pub collection_bulk_export: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SampleCollectionManifest {
    pub schema_version: u8,
    pub dataset_id: String,
    pub is_synthetic_development_fixture: bool,
    pub provenance_banner: String,
    pub access_policy: AccessPolicySummary,
    pub players: Vec<PlayerSummary>,
    pub games: Vec<ArchiveGameRecord>,
}

/// Distinct semantic states for position explorer queries (Section 3 & 11).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PositionQueryStatus {
    Ok,
    NoGamesFound,
    NotYetParsed,
    ExcludedFromDataset,
    NotAnalysed,
}

/// Deduplicated continuation summary: each historical game's FIRST recorded continuation from the
/// queried position counts at most once (repetitions within one game never inflate game_count).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContinuationStat {
    pub move_uci: String,
    pub move_chinese: String,
    pub deduplicated_game_count: usize,
    pub total_occurrence_count: usize,
    pub known_result_count: usize,
    pub red_wins: usize,
    pub draws: usize,
    pub black_wins: usize,
    pub contributing_game_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PositionExplorerResponse {
    pub dataset_id: String,
    pub is_synthetic_fixture: bool,
    pub position_key_sha256: String,
    pub status: PositionQueryStatus,
    pub evaluation_status: PositionQueryStatus,
    pub sample_size_games: usize,
    pub continuations: Vec<ContinuationStat>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CorrectionSubmission {
    pub target_record_id: String,
    pub proposed_change: String,
    pub evidence_url: Option<String>,
    pub note: Option<String>,
}

/// The published dataset release that answered an archive request. Every browse response carries
/// one, so a link or a report can name the exact records it showed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArchiveRelease {
    pub dataset_id: String,
    pub release_id: String,
    pub is_synthetic_fixture: bool,
}

/// Public attribution for a collection or a game: a readable label, an optional public URL, and an
/// optional locator inside the source (a printed page, a source game ID, or a fixture path).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceAttribution {
    pub label: String,
    pub url: Option<String>,
    pub locator: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CollectionSummary {
    pub release: ArchiveRelease,
    pub title: String,
    pub scope: String,
    pub provenance_banner: String,
    pub source: SourceAttribution,
    pub access_policy: AccessPolicySummary,
    pub game_count: usize,
}

/// One row of the game directory: who played, where, when (at its recorded precision), and the
/// recorded result. Moves stay in [`GameDetail`], so browsing never needs bulk replay export.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GameSummary {
    pub game_id: String,
    pub red_player_id: String,
    pub red_display_name: String,
    pub black_player_id: String,
    pub black_display_name: String,
    pub event_name: String,
    pub round: Option<String>,
    pub date_precision: String,
    pub date_display: String,
    pub recorded_result: String,
    pub replay_validity: String,
}

pub const DEFAULT_GAME_PAGE_LIMIT: usize = 25;
pub const MAX_GAME_PAGE_LIMIT: usize = 100;

/// A game-directory query: an optional player filter, an optional folded text match over player
/// names, aliases, and event, and an offset page.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GameQuery {
    #[serde(default)]
    pub player_id: Option<String>,
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub offset: usize,
    #[serde(default = "default_game_page_limit")]
    pub limit: usize,
}

impl Default for GameQuery {
    fn default() -> Self {
        Self {
            player_id: None,
            text: None,
            offset: 0,
            limit: DEFAULT_GAME_PAGE_LIMIT,
        }
    }
}

fn default_game_page_limit() -> usize {
    DEFAULT_GAME_PAGE_LIMIT
}

/// A page of a list whose filter is fixed elsewhere, such as one player's games.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PageRequest {
    #[serde(default)]
    pub offset: usize,
    #[serde(default = "default_game_page_limit")]
    pub limit: usize,
}

impl Default for PageRequest {
    fn default() -> Self {
        Self {
            offset: 0,
            limit: DEFAULT_GAME_PAGE_LIMIT,
        }
    }
}

/// What a game-directory query found. Position analysis states belong to [`PositionQueryStatus`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DirectoryStatus {
    Ok,
    NoGamesFound,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GameSummaryPage {
    pub release: ArchiveRelease,
    pub status: DirectoryStatus,
    pub total: usize,
    pub offset: usize,
    pub limit: usize,
    pub games: Vec<GameSummary>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlayerSearchResponse {
    pub release: ArchiveRelease,
    pub provenance_banner: String,
    pub players: Vec<PlayerSummary>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlayerProfile {
    pub release: ArchiveRelease,
    pub player: PlayerSummary,
    pub games: GameSummaryPage,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GameDetail {
    pub release: ArchiveRelease,
    pub summary: GameSummary,
    pub source: SourceAttribution,
    pub record: ArchiveGameRecord,
}

/// A position-explorer answer with the release that produced it. The explorer's fields stay at the
/// top level, so the JSON is a superset of [`PositionExplorerResponse`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PositionResponse {
    pub release: ArchiveRelease,
    #[serde(flatten)]
    pub explorer: PositionExplorerResponse,
}

/// The public archive read interface. The embedded sample implements it here; the hosted archive
/// service implements it against a published release. Implementations never fetch upstream sources.
pub trait ArchiveProvider {
    fn collection(&self) -> Result<CollectionSummary, ArchiveReadError>;
    fn find_players(&self, query: &str) -> Result<PlayerSearchResponse, ArchiveReadError>;
    fn player_profile(
        &self,
        player_id: &str,
        page: PageRequest,
    ) -> Result<PlayerProfile, ArchiveReadError>;
    fn browse_games(&self, query: &GameQuery) -> Result<GameSummaryPage, ArchiveReadError>;
    fn game_detail(&self, game_id: &str) -> Result<GameDetail, ArchiveReadError>;
    /// Explores the position after `fen`. A blank FEN is an invalid request, never the start position.
    fn explore_position(&self, fen: &str) -> Result<PositionResponse, ArchiveReadError>;
}

/// Why an archive read returned no answer. A missing record, a request the archive cannot answer, and
/// an archive that is not answering stay distinct, so a client never shows one as another.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, thiserror::Error)]
#[serde(tag = "code", content = "message", rename_all = "snake_case")]
pub enum ArchiveReadError {
    #[error("archive record not found: {0}")]
    NotFound(String),
    #[error("invalid archive request: {0}")]
    InvalidRequest(String),
    #[error("archive unavailable: {0}")]
    Unavailable(String),
}

/// Fixture-only fields read beside [`SampleCollectionManifest`] from the same embedded JSON.
#[derive(Debug, Clone, Deserialize)]
struct SampleFixtureExtras {
    collection: SampleCollectionDescription,
    games: Vec<SampleGameExtras>,
}

#[derive(Debug, Clone, Deserialize)]
struct SampleCollectionDescription {
    title: String,
    scope: String,
    source: SourceAttribution,
}

#[derive(Debug, Clone, Deserialize)]
struct SampleGameExtras {
    game_id: String,
    round: Option<String>,
    source: SourceAttribution,
}

#[derive(Debug, Clone)]
pub struct SampleArchiveProvider {
    manifest: SampleCollectionManifest,
    extras: SampleFixtureExtras,
    release: ArchiveRelease,
}

impl Default for SampleArchiveProvider {
    fn default() -> Self {
        Self::new_embedded()
    }
}

impl SampleArchiveProvider {
    pub fn new_embedded() -> Self {
        let manifest: SampleCollectionManifest = serde_json::from_str(EMBEDDED_SAMPLE_COLLECTION)
            .expect("valid embedded sample archive fixture");
        let extras: SampleFixtureExtras = serde_json::from_str(EMBEDDED_SAMPLE_COLLECTION)
            .expect("valid embedded sample archive fixture extras");
        // The release ID names the fixture's exact bytes, so two builds that show different sample
        // records never claim the same release.
        let digest = hex::encode(Sha256::digest(EMBEDDED_SAMPLE_COLLECTION.as_bytes()));
        let release = ArchiveRelease {
            dataset_id: manifest.dataset_id.clone(),
            release_id: format!("{}@sha256:{digest}", manifest.dataset_id),
            is_synthetic_fixture: manifest.is_synthetic_development_fixture,
        };
        Self {
            manifest,
            extras,
            release,
        }
    }

    pub fn release(&self) -> &ArchiveRelease {
        &self.release
    }

    fn summarize(&self, game: &ArchiveGameRecord) -> GameSummary {
        let name_of = |id: &str| {
            self.manifest
                .players
                .iter()
                .find(|p| p.player_id == id)
                .map(|p| p.primary_display_name.clone())
                .unwrap_or_else(|| id.to_string())
        };
        let round = self
            .extras
            .games
            .iter()
            .find(|g| g.game_id == game.game_id)
            .and_then(|g| g.round.clone());
        GameSummary {
            game_id: game.game_id.clone(),
            red_player_id: game.red_player_id.clone(),
            red_display_name: name_of(&game.red_player_id),
            black_player_id: game.black_player_id.clone(),
            black_display_name: name_of(&game.black_player_id),
            event_name: game.event_name.clone(),
            round,
            date_precision: game.date_precision.clone(),
            date_display: game.date_display.clone(),
            recorded_result: game.recorded_result.clone(),
            replay_validity: game.replay_validity.clone(),
        }
    }

    fn player_matches_text(&self, player_id: &str, folded: &str) -> bool {
        self.manifest
            .players
            .iter()
            .find(|p| p.player_id == player_id)
            .is_some_and(|p| {
                fold_search_key(&p.primary_display_name).contains(folded)
                    || fold_search_key(&p.native_display_name).contains(folded)
                    || p.aliases
                        .iter()
                        .any(|a| fold_search_key(&a.text).contains(folded))
            })
    }
}

impl ArchiveProvider for SampleArchiveProvider {
    fn collection(&self) -> Result<CollectionSummary, ArchiveReadError> {
        Ok(CollectionSummary {
            release: self.release.clone(),
            title: self.extras.collection.title.clone(),
            scope: self.extras.collection.scope.clone(),
            provenance_banner: self.manifest.provenance_banner.clone(),
            source: self.extras.collection.source.clone(),
            access_policy: self.manifest.access_policy.clone(),
            game_count: self.manifest.games.len(),
        })
    }

    fn find_players(&self, query: &str) -> Result<PlayerSearchResponse, ArchiveReadError> {
        Ok(PlayerSearchResponse {
            release: self.release.clone(),
            provenance_banner: self.manifest.provenance_banner.clone(),
            players: self.search_players(query),
        })
    }

    fn player_profile(
        &self,
        player_id: &str,
        page: PageRequest,
    ) -> Result<PlayerProfile, ArchiveReadError> {
        let player = self
            .manifest
            .players
            .iter()
            .find(|p| p.player_id == player_id)
            .ok_or_else(|| ArchiveReadError::NotFound(format!("player {player_id}")))?
            .clone();
        let games = self.browse_games(&GameQuery {
            player_id: Some(player_id.to_string()),
            text: None,
            offset: page.offset,
            limit: page.limit,
        })?;
        Ok(PlayerProfile {
            release: self.release.clone(),
            player,
            games,
        })
    }

    fn browse_games(&self, query: &GameQuery) -> Result<GameSummaryPage, ArchiveReadError> {
        let folded = query
            .text
            .as_deref()
            .map(fold_search_key)
            .filter(|t| !t.is_empty());
        let matching: Vec<&ArchiveGameRecord> = self
            .manifest
            .games
            .iter()
            .filter(|g| {
                query
                    .player_id
                    .as_deref()
                    .is_none_or(|id| g.red_player_id == id || g.black_player_id == id)
            })
            .filter(|g| {
                folded.as_deref().is_none_or(|t| {
                    fold_search_key(&g.event_name).contains(t)
                        || self.player_matches_text(&g.red_player_id, t)
                        || self.player_matches_text(&g.black_player_id, t)
                })
            })
            .collect();
        let limit = query.limit.clamp(1, MAX_GAME_PAGE_LIMIT);
        let games = matching
            .iter()
            .skip(query.offset)
            .take(limit)
            .map(|g| self.summarize(g))
            .collect();
        Ok(GameSummaryPage {
            release: self.release.clone(),
            status: if matching.is_empty() {
                DirectoryStatus::NoGamesFound
            } else {
                DirectoryStatus::Ok
            },
            total: matching.len(),
            offset: query.offset,
            limit,
            games,
        })
    }

    fn game_detail(&self, game_id: &str) -> Result<GameDetail, ArchiveReadError> {
        let record = self
            .get_game(game_id)
            .ok_or_else(|| ArchiveReadError::NotFound(format!("game {game_id}")))?
            .clone();
        let source = self
            .extras
            .games
            .iter()
            .find(|g| g.game_id == game_id)
            .map(|g| g.source.clone())
            .unwrap_or_else(|| self.extras.collection.source.clone());
        Ok(GameDetail {
            release: self.release.clone(),
            summary: self.summarize(&record),
            source,
            record,
        })
    }

    fn explore_position(&self, fen: &str) -> Result<PositionResponse, ArchiveReadError> {
        if fen.trim().is_empty() {
            return Err(ArchiveReadError::InvalidRequest(
                "a position FEN is required".to_string(),
            ));
        }
        Ok(PositionResponse {
            release: self.release.clone(),
            explorer: self
                .query_position(fen)
                .map_err(ArchiveReadError::InvalidRequest)?,
        })
    }
}

impl SampleArchiveProvider {
    pub fn manifest(&self) -> &SampleCollectionManifest {
        &self.manifest
    }

    /// Searches players by case/space/punctuation-folded alias or exact CJK prefix without
    /// merging distinct people who happen to share a name (`林明` returns both disambiguated entries).
    pub fn search_players(&self, query: &str) -> Vec<PlayerSummary> {
        let q = fold_search_key(query);
        if q.is_empty() {
            return self.manifest.players.clone();
        }
        self.manifest
            .players
            .iter()
            .filter(|p| {
                fold_search_key(&p.primary_display_name).contains(&q)
                    || fold_search_key(&p.native_display_name).contains(&q)
                    || p.aliases
                        .iter()
                        .any(|a| fold_search_key(&a.text).contains(&q))
            })
            .cloned()
            .collect()
    }

    pub fn get_game(&self, game_id: &str) -> Option<&ArchiveGameRecord> {
        self.manifest.games.iter().find(|g| g.game_id == game_id)
    }

    /// Queries deduplicated first-continuation statistics for `fen` within the sample dataset.
    /// Never makes upstream network requests.
    pub fn query_position(&self, fen: &str) -> Result<PositionExplorerResponse, String> {
        let target_state =
            GameState::from_fen(fen, ADOPTED_RULES_PROFILE).map_err(|e| e.to_string())?;
        let target_key = target_state.board().position_key();
        Ok(self.query_position_by_key(&target_key))
    }

    pub fn query_position_by_key(&self, target_key: &PositionKey) -> PositionExplorerResponse {
        #[derive(Default)]
        struct Acc {
            move_chinese: String,
            dedup_games: usize,
            occurrences: usize,
            known_results: usize,
            red_wins: usize,
            draws: usize,
            black_wins: usize,
            game_ids: Vec<String>,
        }

        let mut by_move: HashMap<String, Acc> = HashMap::new();
        let mut matched_games = 0usize;

        for game_rec in &self.manifest.games {
            // Default human-historical filter excludes engine self-play and compositions (Section 11).
            if game_rec.participant_kind == "engine" || game_rec.game_context == "composition" {
                continue;
            }
            let Ok(mut state) = GameState::from_fen(&game_rec.initial_fen, ADOPTED_RULES_PROFILE)
            else {
                continue;
            };

            let mut seen_in_this_game = false;
            let mut seen_moves_in_game: HashSet<String> = HashSet::new();

            for uci in &game_rec.moves_uci {
                let cur_key = state.board().position_key();
                let Ok(mv) = Move::from_uci_or_iccs(uci) else {
                    break;
                };
                if cur_key.sha256_hex == target_key.sha256_hex
                    && cur_key.canonical_bytes == target_key.canonical_bytes
                {
                    let zh = format_chinese_move(state.board(), mv, false)
                        .unwrap_or_else(|_| mv.to_uci());
                    let entry = by_move.entry(mv.to_uci()).or_insert_with(|| Acc {
                        move_chinese: zh,
                        ..Default::default()
                    });
                    entry.occurrences += 1;

                    // Count each deduplicated game's FIRST continuation from the queried position once!
                    if !seen_in_this_game && seen_moves_in_game.insert(mv.to_uci()) {
                        seen_in_this_game = true;
                        matched_games += 1;
                        entry.dedup_games += 1;
                        entry.game_ids.push(game_rec.game_id.clone());
                        match game_rec.recorded_result.as_str() {
                            "red_win" => {
                                entry.known_results += 1;
                                entry.red_wins += 1;
                            }
                            "black_win" => {
                                entry.known_results += 1;
                                entry.black_wins += 1;
                            }
                            "draw" => {
                                entry.known_results += 1;
                                entry.draws += 1;
                            }
                            // Unknown or ongoing results are NEVER silently counted as draws!
                            _ => {}
                        }
                    }
                }
                if state.apply_move(mv).is_err() {
                    break;
                }
            }
        }

        let mut continuations: Vec<ContinuationStat> = by_move
            .into_iter()
            .map(|(move_uci, acc)| ContinuationStat {
                move_uci,
                move_chinese: acc.move_chinese,
                deduplicated_game_count: acc.dedup_games,
                total_occurrence_count: acc.occurrences,
                known_result_count: acc.known_results,
                red_wins: acc.red_wins,
                draws: acc.draws,
                black_wins: acc.black_wins,
                contributing_game_ids: acc.game_ids,
            })
            .collect();
        continuations.sort_by(|a, b| {
            b.deduplicated_game_count
                .cmp(&a.deduplicated_game_count)
                .then_with(|| a.move_uci.cmp(&b.move_uci))
        });

        let status = if continuations.is_empty() {
            PositionQueryStatus::NoGamesFound
        } else {
            PositionQueryStatus::Ok
        };

        let _ = NotationDialect::Uci;

        PositionExplorerResponse {
            dataset_id: self.manifest.dataset_id.clone(),
            is_synthetic_fixture: self.manifest.is_synthetic_development_fixture,
            position_key_sha256: target_key.sha256_hex.clone(),
            status,
            evaluation_status: PositionQueryStatus::NotAnalysed,
            sample_size_games: matched_games,
            continuations,
        }
    }
}

fn fold_search_key(s: &str) -> String {
    s.chars()
        .filter(|c| !c.is_whitespace() && !matches!(c, ',' | '.' | '-' | '_'))
        .flat_map(|c| c.to_lowercase())
        .collect()
}

pub fn crate_ready() -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use xiangqi_core::STARTING_FEN;

    #[test]
    fn sample_provider_disambiguates_same_name_and_pairs_wxf_aliases() {
        let provider = SampleArchiveProvider::new_embedded();
        let chiu = provider.search_players("chiu yu kuen");
        assert_eq!(chiu.len(), 1);
        assert_eq!(chiu[0].native_display_name, "赵汝权");

        let same_name = provider.search_players("林明");
        assert_eq!(
            same_name.len(),
            2,
            "fuzzy/alias search must never merge same-name players"
        );
    }

    #[test]
    fn sample_games_marked_complete_replay_every_recorded_move() {
        let provider = SampleArchiveProvider::new_embedded();
        for game in &provider.manifest().games {
            if game.replay_validity != "valid_prefix_complete" {
                continue;
            }
            let mut state = GameState::from_fen(&game.initial_fen, ADOPTED_RULES_PROFILE).unwrap();
            for (ply, uci) in game.moves_uci.iter().enumerate() {
                let mv = Move::from_uci_or_iccs(uci).unwrap();
                assert!(
                    state.apply_move(mv).is_ok(),
                    "{} ply {} ({uci}) does not replay",
                    game.game_id,
                    ply + 1
                );
            }
            assert_eq!(
                game.moves_uci.len(),
                game.moves_chinese.len(),
                "{}",
                game.game_id
            );
        }
    }

    #[test]
    fn sample_provider_counts_deduplicated_first_continuations() {
        let provider = SampleArchiveProvider::new_embedded();
        let resp = provider.query_position(STARTING_FEN).unwrap();
        assert_eq!(resp.status, PositionQueryStatus::Ok);
        assert_eq!(resp.sample_size_games, 2);
        assert_eq!(resp.continuations.len(), 1);
        assert_eq!(resp.continuations[0].move_uci, "h2e2");
        assert_eq!(resp.continuations[0].move_chinese, "炮二平五");
        assert_eq!(resp.continuations[0].red_wins, 1);
        assert_eq!(resp.continuations[0].draws, 1);
        assert_eq!(resp.continuations[0].black_wins, 0);
    }

    #[test]
    fn sample_collection_names_its_release_and_synthetic_source() {
        let provider = SampleArchiveProvider::new_embedded();
        let collection = provider.collection().unwrap();
        assert_eq!(
            collection.release.dataset_id,
            "jade-public-sample-fixtures-v1"
        );
        let digest = hex::encode(Sha256::digest(EMBEDDED_SAMPLE_COLLECTION.as_bytes()));
        assert_eq!(
            collection.release.release_id,
            format!("jade-public-sample-fixtures-v1@sha256:{digest}")
        );
        assert!(collection.release.is_synthetic_fixture);
        assert_eq!(collection.game_count, 2);
        assert_eq!(collection.source.url, None);
        assert_eq!(
            provider.find_players("").unwrap().release,
            collection.release
        );
    }

    #[test]
    fn sample_browse_pages_and_filters_games_by_player_and_text() {
        let provider = SampleArchiveProvider::new_embedded();
        let all = provider.browse_games(&GameQuery::default()).unwrap();
        assert_eq!(all.status, DirectoryStatus::Ok);
        assert_eq!(all.total, 2);
        assert_eq!(all.games[1].game_id, "sample-game-screen-horse-002");
        assert_eq!(all.games[0].red_display_name, "Chiu, Yu Kuen");

        let second_page = provider
            .browse_games(&GameQuery {
                offset: 1,
                limit: 1,
                ..GameQuery::default()
            })
            .unwrap();
        assert_eq!(second_page.total, 2);
        assert_eq!(second_page.games.len(), 1);
        assert_eq!(second_page.games[0].game_id, "sample-game-screen-horse-002");

        let by_player = provider
            .browse_games(&GameQuery {
                player_id: Some("sample-player-003".to_string()),
                ..GameQuery::default()
            })
            .unwrap();
        assert_eq!(by_player.total, 1);
        assert_eq!(by_player.games[0].game_id, "sample-game-screen-horse-002");

        // A reviewed alias in another script finds the game without being shown beside every name.
        let by_alias = provider
            .browse_games(&GameQuery {
                text: Some("趙汝權".to_string()),
                ..GameQuery::default()
            })
            .unwrap();
        assert_eq!(by_alias.total, 1);
        assert_eq!(by_alias.games[0].game_id, "sample-game-central-cannon-001");

        let none = provider
            .browse_games(&GameQuery {
                text: Some("no such player".to_string()),
                ..GameQuery::default()
            })
            .unwrap();
        assert_eq!(none.status, DirectoryStatus::NoGamesFound);
        assert!(none.games.is_empty());
    }

    #[test]
    fn sample_player_profile_lists_that_players_games() {
        let provider = SampleArchiveProvider::new_embedded();
        let profile = provider
            .player_profile("sample-player-001", PageRequest::default())
            .unwrap();
        assert_eq!(profile.player.native_display_name, "赵汝权");
        assert_eq!(profile.games.total, 1);
        assert_eq!(profile.games.games[0].red_player_id, "sample-player-001");
        assert!(matches!(
            provider.player_profile("no-such-player", PageRequest::default()),
            Err(ArchiveReadError::NotFound(_))
        ));
    }

    #[test]
    fn sample_game_detail_keeps_record_source_and_release_together() {
        let provider = SampleArchiveProvider::new_embedded();
        let detail = provider
            .game_detail("sample-game-screen-horse-002")
            .unwrap();
        assert_eq!(detail.summary.game_id, detail.record.game_id);
        assert_eq!(
            detail.source.locator.as_deref(),
            Some("sample-game-screen-horse-002")
        );
        assert_eq!(&detail.release, provider.release());
        assert!(matches!(
            provider.game_detail("no-such-game"),
            Err(ArchiveReadError::NotFound(_))
        ));
    }

    #[test]
    fn position_response_json_keeps_the_explorer_fields_at_top_level() {
        let provider = SampleArchiveProvider::new_embedded();
        let resp = provider.explore_position(STARTING_FEN).unwrap();
        let json = serde_json::to_value(&resp).unwrap();
        assert_eq!(json["dataset_id"], "jade-public-sample-fixtures-v1");
        assert_eq!(json["continuations"][0]["move_uci"], "h2e2");
        assert_eq!(json["release"]["release_id"], resp.release.release_id);
        let back: PositionResponse = serde_json::from_value(json).unwrap();
        assert_eq!(back, resp);
    }

    #[test]
    fn blank_or_malformed_fen_is_an_invalid_request() {
        let provider = SampleArchiveProvider::new_embedded();
        assert!(matches!(
            provider.explore_position("  "),
            Err(ArchiveReadError::InvalidRequest(_))
        ));
        assert!(matches!(
            provider.explore_position("not a fen"),
            Err(ArchiveReadError::InvalidRequest(_))
        ));
        let json = serde_json::to_value(ArchiveReadError::NotFound("game x".into())).unwrap();
        assert_eq!(json["code"], "not_found");
    }

    #[test]
    fn match_privacy_defaults_private_and_enforces_both_participants() {
        let mut policy = MatchPrivacyConsent::default();
        assert_eq!(policy.effective_visibility(), Visibility::Private);
        assert!(!policy.eligible_for_internal_training());

        policy.red_visibility = Visibility::Public;
        policy.black_visibility = Visibility::Unlisted;
        assert_eq!(policy.effective_visibility(), Visibility::Unlisted);

        policy.red_allow_internal_training = true;
        assert!(!policy.eligible_for_internal_training());
        policy.black_allow_internal_training = true;
        assert!(policy.eligible_for_internal_training());
        assert!(!policy.eligible_for_dataset_release());
    }
}
