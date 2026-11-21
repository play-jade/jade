use serde::{Deserialize, Serialize};
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

#[derive(Debug, Clone)]
pub struct SampleArchiveProvider {
    manifest: SampleCollectionManifest,
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
        Self { manifest }
    }

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
