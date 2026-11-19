use serde::{Deserialize, Serialize};
use thiserror::Error;
use xiangqi_core::{ACTION_SPACE_SIZE, ADOPTED_RULES_PROFILE, GameState, Role, Side, Square};

pub const OBSERVATION_SCHEMA_V1: &str = "jade-obs-16x10x9-v1";
pub const ACTION_SCHEMA_V1: &str = "from_sq_90_plus_to_sq_8100_v1";
pub const OBSERVATION_PLANES: usize = 16;

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum InferenceContractError {
    #[error("incompatible rules profile: expected {ADOPTED_RULES_PROFILE}, got {0}")]
    IncompatibleRulesProfile(String),
    #[error("forbidden training/internal field present in public inference bundle: {0}")]
    ForbiddenTrainingStatePresent(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ResearchTrack {
    RulesOnlySelfPlay,
    CorpusInformed,
    TeacherAssisted,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReleasedInferenceManifest {
    pub bundle_id: String,
    pub track: ResearchTrack,
    pub architecture: String,
    pub observation_schema: String,
    pub action_schema: String,
    pub rules_profile: String,
    pub weights_sha256: String,
    pub supported_backends: Vec<String>,
    /// Explicitly records what the model observation omits relative to complete adjudication state (Section 13).
    pub documented_information_gap: String,
}

impl ReleasedInferenceManifest {
    pub fn validate_for_public_release(
        &self,
        raw_json: &serde_json::Value,
    ) -> Result<(), InferenceContractError> {
        if self.rules_profile != ADOPTED_RULES_PROFILE {
            return Err(InferenceContractError::IncompatibleRulesProfile(
                self.rules_profile.clone(),
            ));
        }
        for forbidden in [
            "optimizer_state",
            "replay_buffer",
            "credentials",
            "internal_storage_path",
        ] {
            if raw_json.get(forbidden).is_some() {
                return Err(InferenceContractError::ForbiddenTrainingStatePresent(
                    forbidden.to_string(),
                ));
            }
        }
        Ok(())
    }
}

/// Encodes `GameState` into `16 × 10 × 9 = 1,440` f32 features and an `8,100`-element legal action mask.
/// Planes 0..7: side-to-move pieces (`General..Soldier`), Planes 7..14: opponent pieces,
/// Plane 14: side-to-move indicator (`1.0` Red, `0.0` Black),
/// Plane 15: normalized `effective_non_capture_plies / 120.0`.
pub struct EncodedPosition {
    pub tensor_16x10x9: Vec<f32>,
    pub legal_action_mask: Vec<bool>,
}

pub fn encode_position_v1(state: &GameState) -> EncodedPosition {
    let board = state.board();
    let stm = board.side_to_move();
    let mut tensor = vec![0.0f32; OBSERVATION_PLANES * 90];

    for idx in 0..90u8 {
        let sq = Square::from_index(idx).unwrap();
        if let Some(p) = board.piece_at(sq) {
            let role_offset = match p.role {
                Role::General => 0,
                Role::Advisor => 1,
                Role::Elephant => 2,
                Role::Chariot => 3,
                Role::Horse => 4,
                Role::Cannon => 5,
                Role::Soldier => 6,
            };
            let plane = if p.side == stm {
                role_offset
            } else {
                7 + role_offset
            };
            tensor[plane * 90 + sq.index()] = 1.0;
        }
    }

    if stm == Side::Red {
        for cell in &mut tensor[14 * 90..15 * 90] {
            *cell = 1.0;
        }
    }
    let progress = (state.effective_non_capture_plies() as f32 / 120.0).clamp(0.0, 1.0);
    for cell in &mut tensor[15 * 90..16 * 90] {
        *cell = progress;
    }

    let mut mask = vec![false; ACTION_SPACE_SIZE];
    for mv in board.legal_moves() {
        mask[mv.action_index() as usize] = true;
    }

    EncodedPosition {
        tensor_16x10x9: tensor,
        legal_action_mask: mask,
    }
}

pub fn crate_ready() -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_8100_legal_mask_and_rejects_training_state_leakage() {
        let game = GameState::default();
        let enc = encode_position_v1(&game);
        assert_eq!(enc.tensor_16x10x9.len(), 16 * 90);
        assert_eq!(enc.legal_action_mask.len(), 8100);
        assert_eq!(enc.legal_action_mask.iter().filter(|&&b| b).count(), 44);

        let manifest = ReleasedInferenceManifest {
            bundle_id: "jade-rules-only-gen0-proof".to_string(),
            track: ResearchTrack::RulesOnlySelfPlay,
            architecture: "resnet-6x64-policy-wdl".to_string(),
            observation_schema: OBSERVATION_SCHEMA_V1.to_string(),
            action_schema: ACTION_SCHEMA_V1.to_string(),
            rules_profile: ADOPTED_RULES_PROFILE.to_string(),
            weights_sha256: "00".repeat(32),
            supported_backends: vec!["wgpu".to_string(), "ndarray-cpu".to_string()],
            documented_information_gap: "Single-frame placement + non-capture ratio omits full cycle chase history; MCTS environment retains exact GameState adjudication.".to_string(),
        };
        let clean_json = serde_json::to_value(&manifest).unwrap();
        assert!(manifest.validate_for_public_release(&clean_json).is_ok());

        let leaked = serde_json::json!({
            "optimizer_state": "adamw_moments.bin"
        });
        assert!(manifest.validate_for_public_release(&leaked).is_err());
    }
}
