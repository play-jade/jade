use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::process::Stdio;
use std::time::{Duration, Instant};
use thiserror::Error;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::time::timeout;
use xiangqi_core::{ADOPTED_RULES_PROFILE, GameState, Move, Side};

#[derive(Debug, Error)]
pub enum EngineHostError {
    #[error("engine startup failed: {0}")]
    StartupFailed(String),
    #[error("engine timed out during {phase}")]
    Timeout { phase: &'static str },
    #[error("engine process exited or crashed: {0}")]
    ProcessCrashed(String),
    #[error("engine proposed illegal move '{move_uci}': {reason}")]
    IllegalEngineMove { move_uci: String, reason: String },
    #[error("invalid position for analysis: {0}")]
    InvalidPosition(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchBudget {
    pub depth: Option<u32>,
    pub movetime_ms: Option<u64>,
    pub nodes: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchRequest {
    pub search_id: String,
    pub state_id: String,
    pub engine_id: String,
    pub binary_sha256: String,
    pub network_sha256: Option<String>,
    pub rules_profile: String,
    pub initial_fen: String,
    pub moves_uci: Vec<String>,
    pub multipv: u32,
    pub threads: u32,
    pub hash_mb: u32,
    pub budget: SearchBudget,
}

impl SearchRequest {
    pub fn effective_config_hash(&self) -> String {
        let mut h = Sha256::new();
        h.update(self.engine_id.as_bytes());
        h.update(b":");
        h.update(self.binary_sha256.as_bytes());
        h.update(b":");
        if let Some(ref net) = self.network_sha256 {
            h.update(net.as_bytes());
        }
        h.update(format!(
            ":{}:{}:{}:{}",
            self.rules_profile, self.multipv, self.threads, self.hash_mb
        ));
        hex::encode(h.finalize())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ScoreKind {
    Cp,
    Mate,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NormalizedInfoLine {
    pub search_id: String,
    pub state_id: String,
    pub multipv: u32,
    pub depth: u32,
    pub seldepth: Option<u32>,
    pub score_kind: ScoreKind,
    /// Score normalized to Red's perspective (`+` favors Red, `-` favors Black).
    pub score_red_perspective: i32,
    pub wdl: Option<[u32; 3]>,
    pub nodes: u64,
    pub nps: u64,
    pub elapsed_ms: u64,
    pub validated_pv_uci: Vec<String>,
    pub raw_line: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EvaluationManifest {
    pub search_id: String,
    pub state_id: String,
    pub engine_id: String,
    pub reported_engine_name: Option<String>,
    pub binary_sha256: String,
    pub network_sha256: Option<String>,
    pub rules_profile: String,
    pub effective_config_hash: String,
    pub threads: u32,
    pub hash_mb: u32,
    pub multipv: u32,
    pub budget: SearchBudget,
    pub cancelled: bool,
    pub elapsed_ms: u64,
    pub bestmove_uci: Option<String>,
    pub infos: Vec<NormalizedInfoLine>,
}

/// Parses one UCI `info` line independently of `bestmove` and validates the PV prefix
/// against `xiangqi-core` so illegal engine tokens never corrupt the board.
pub fn parse_uci_info_line(
    line: &str,
    search_id: &str,
    state_id: &str,
    root_state: &GameState,
) -> Option<NormalizedInfoLine> {
    let trimmed = line.trim();
    if !trimmed.starts_with("info ") {
        return None;
    }
    let tokens: Vec<&str> = trimmed.split_whitespace().collect();
    let mut depth = 0u32;
    let mut seldepth = None;
    let mut multipv = 1u32;
    let mut score_kind = ScoreKind::Cp;
    let mut raw_score = 0i32;
    let mut has_score = false;
    let mut wdl = None;
    let mut nodes = 0u64;
    let mut nps = 0u64;
    let mut elapsed_ms = 0u64;
    let mut pv_raw: Vec<&str> = Vec::new();

    let mut i = 1;
    while i < tokens.len() {
        match tokens[i] {
            "depth" if i + 1 < tokens.len() => {
                depth = tokens[i + 1].parse().unwrap_or(0);
                i += 2;
            }
            "seldepth" if i + 1 < tokens.len() => {
                seldepth = tokens[i + 1].parse().ok();
                i += 2;
            }
            "multipv" if i + 1 < tokens.len() => {
                multipv = tokens[i + 1].parse().unwrap_or(1);
                i += 2;
            }
            "score" if i + 2 < tokens.len() => match tokens[i + 1] {
                "cp" => {
                    score_kind = ScoreKind::Cp;
                    raw_score = tokens[i + 2].parse().unwrap_or(0);
                    has_score = true;
                    i += 3;
                }
                "mate" => {
                    score_kind = ScoreKind::Mate;
                    raw_score = tokens[i + 2].parse().unwrap_or(0);
                    has_score = true;
                    i += 3;
                }
                _ => i += 1,
            },
            "wdl" if i + 3 < tokens.len() => {
                if let (Ok(w), Ok(d), Ok(l)) = (
                    tokens[i + 1].parse(),
                    tokens[i + 2].parse(),
                    tokens[i + 3].parse(),
                ) {
                    wdl = Some([w, d, l]);
                }
                i += 4;
            }
            "nodes" if i + 1 < tokens.len() => {
                nodes = tokens[i + 1].parse().unwrap_or(0);
                i += 2;
            }
            "nps" if i + 1 < tokens.len() => {
                nps = tokens[i + 1].parse().unwrap_or(0);
                i += 2;
            }
            "time" if i + 1 < tokens.len() => {
                elapsed_ms = tokens[i + 1].parse().unwrap_or(0);
                i += 2;
            }
            "pv" => {
                pv_raw.extend_from_slice(&tokens[i + 1..]);
                break;
            }
            "string" => break,
            _ => i += 1,
        }
    }

    if !has_score && pv_raw.is_empty() {
        return None;
    }

    // UCI reports score from side-to-move's perspective; normalize to Red perspective!
    let sign = match root_state.side_to_move() {
        Side::Red => 1,
        Side::Black => -1,
    };
    let score_red_perspective = raw_score.saturating_mul(sign);

    // Validate PV moves sequentially through canonical `xiangqi-core`!
    let mut sim = root_state.clone();
    let mut validated_pv_uci = Vec::with_capacity(pv_raw.len());
    for tok in pv_raw {
        let Ok(mv) = Move::from_uci_or_iccs(tok) else {
            break;
        };
        if sim.apply_move(mv).is_err() {
            break;
        }
        validated_pv_uci.push(mv.to_uci());
    }

    Some(NormalizedInfoLine {
        search_id: search_id.to_string(),
        state_id: state_id.to_string(),
        multipv,
        depth,
        seldepth,
        score_kind,
        score_red_perspective,
        wdl,
        nodes,
        nps,
        elapsed_ms,
        validated_pv_uci,
        raw_line: trimmed.to_string(),
    })
}

pub fn parse_uci_bestmove_line(line: &str) -> Option<Option<String>> {
    let trimmed = line.trim();
    if !trimmed.starts_with("bestmove") {
        return None;
    }
    let mut parts = trimmed.split_whitespace();
    let _ = parts.next()?;
    match parts.next() {
        Some("(none)") | Some("none") | Some("0000") | None => Some(None),
        Some(mv) => Some(Some(mv.to_string())),
    }
}

/// Supervised desktop/worker UCI process host with line buffering for arbitrary stream
/// fragmentation, bounded startup/cancellation/shutdown deadlines, and crash isolation.
pub struct SupervisedUciProcess {
    child: Child,
    stdin: ChildStdin,
    stdout_lines: tokio::io::Lines<BufReader<ChildStdout>>,
    reported_name: Option<String>,
    discovered_options: Vec<String>,
}

impl SupervisedUciProcess {
    pub async fn spawn(
        binary_path: &str,
        startup_timeout: Duration,
    ) -> Result<Self, EngineHostError> {
        let mut child = Command::new(binary_path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| EngineHostError::StartupFailed(format!("{binary_path}: {e}")))?;

        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| EngineHostError::StartupFailed("missing stdin".to_string()))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| EngineHostError::StartupFailed("missing stdout".to_string()))?;

        let mut proc = Self {
            child,
            stdin,
            stdout_lines: BufReader::new(stdout).lines(),
            reported_name: None,
            discovered_options: Vec::new(),
        };

        proc.send_line("uci").await?;
        timeout(startup_timeout, async {
            while let Some(line) = proc
                .stdout_lines
                .next_line()
                .await
                .map_err(|e| EngineHostError::ProcessCrashed(e.to_string()))?
            {
                let t = line.trim();
                if let Some(rest) = t.strip_prefix("id name ") {
                    proc.reported_name = Some(rest.trim().to_string());
                } else if t.starts_with("option name ") {
                    proc.discovered_options.push(t.to_string());
                } else if t == "uciok" {
                    return Ok(());
                }
            }
            Err(EngineHostError::ProcessCrashed(
                "EOF before uciok".to_string(),
            ))
        })
        .await
        .map_err(|_| EngineHostError::Timeout { phase: "uciok" })??;

        proc.ensure_ready(startup_timeout).await?;
        Ok(proc)
    }

    pub fn reported_name(&self) -> Option<&str> {
        self.reported_name.as_deref()
    }

    pub fn discovered_options(&self) -> &[String] {
        &self.discovered_options
    }

    async fn send_line(&mut self, cmd: &str) -> Result<(), EngineHostError> {
        self.stdin
            .write_all(format!("{cmd}\n").as_bytes())
            .await
            .map_err(|e| EngineHostError::ProcessCrashed(e.to_string()))?;
        self.stdin
            .flush()
            .await
            .map_err(|e| EngineHostError::ProcessCrashed(e.to_string()))
    }

    pub async fn ensure_ready(&mut self, ready_timeout: Duration) -> Result<(), EngineHostError> {
        self.send_line("isready").await?;
        timeout(ready_timeout, async {
            while let Some(line) = self
                .stdout_lines
                .next_line()
                .await
                .map_err(|e| EngineHostError::ProcessCrashed(e.to_string()))?
            {
                if line.trim() == "readyok" {
                    return Ok(());
                }
            }
            Err(EngineHostError::ProcessCrashed(
                "EOF before readyok".to_string(),
            ))
        })
        .await
        .map_err(|_| EngineHostError::Timeout { phase: "isready" })?
    }

    pub async fn run_search(
        &mut self,
        req: &SearchRequest,
        cancel_after: Option<Duration>,
        hard_deadline: Duration,
    ) -> Result<EvaluationManifest, EngineHostError> {
        let mut root_state = GameState::from_fen(&req.initial_fen, ADOPTED_RULES_PROFILE)
            .map_err(|e| EngineHostError::InvalidPosition(e.to_string()))?;
        for uci in &req.moves_uci {
            let mv = Move::from_uci_or_iccs(uci)
                .map_err(|e| EngineHostError::InvalidPosition(e.to_string()))?;
            root_state
                .apply_move(mv)
                .map_err(|e| EngineHostError::InvalidPosition(e.to_string()))?;
        }

        // Only send UCI options the engine actually advertised (graceful unsupported-option handling).
        if self.supports_option("Threads") {
            self.send_line(&format!(
                "setoption name Threads value {}",
                req.threads.max(1)
            ))
            .await?;
        }
        if self.supports_option("Hash") {
            self.send_line(&format!("setoption name Hash value {}", req.hash_mb.max(1)))
                .await?;
        }
        if self.supports_option("MultiPV") && req.multipv > 1 {
            self.send_line(&format!("setoption name MultiPV value {}", req.multipv))
                .await?;
        }
        self.ensure_ready(Duration::from_secs(3)).await?;

        let pos_cmd = if req.moves_uci.is_empty() {
            format!("position fen {}", req.initial_fen)
        } else {
            format!(
                "position fen {} moves {}",
                req.initial_fen,
                req.moves_uci.join(" ")
            )
        };
        self.send_line(&pos_cmd).await?;

        let go_cmd = if let Some(ms) = req.budget.movetime_ms {
            format!("go movetime {ms}")
        } else if let Some(nodes) = req.budget.nodes {
            format!("go nodes {nodes}")
        } else if let Some(depth) = req.budget.depth {
            format!("go depth {depth}")
        } else {
            "go infinite".to_string()
        };

        let started = Instant::now();
        self.send_line(&go_cmd).await?;

        let mut infos = Vec::new();
        let mut cancelled = false;
        let mut stop_sent = false;
        let mut bestmove_uci = None;

        let search_res = timeout(hard_deadline, async {
            loop {
                if let Some(cancel_dur) = cancel_after {
                    if !stop_sent && started.elapsed() >= cancel_dur {
                        self.send_line("stop").await?;
                        stop_sent = true;
                        cancelled = true;
                    }
                    let wait_left = cancel_dur.saturating_sub(started.elapsed());
                    if !stop_sent {
                        match timeout(
                            wait_left.max(Duration::from_millis(1)),
                            self.stdout_lines.next_line(),
                        )
                        .await
                        {
                            Ok(Ok(Some(line))) => {
                                if let Some(bm) =
                                    Self::process_line(&line, req, &root_state, &mut infos)?
                                {
                                    bestmove_uci = bm;
                                    return Ok::<(), EngineHostError>(());
                                }
                            }
                            Ok(Ok(None)) => {
                                return Err(EngineHostError::ProcessCrashed(
                                    "EOF during search".to_string(),
                                ));
                            }
                            Ok(Err(e)) => {
                                return Err(EngineHostError::ProcessCrashed(e.to_string()));
                            }
                            Err(_) => {
                                self.send_line("stop").await?;
                                stop_sent = true;
                                cancelled = true;
                            }
                        }
                        continue;
                    }
                }

                match self.stdout_lines.next_line().await {
                    Ok(Some(line)) => {
                        if let Some(bm) = Self::process_line(&line, req, &root_state, &mut infos)? {
                            bestmove_uci = bm;
                            return Ok(());
                        }
                    }
                    Ok(None) => {
                        return Err(EngineHostError::ProcessCrashed(
                            "EOF during search".to_string(),
                        ));
                    }
                    Err(e) => {
                        return Err(EngineHostError::ProcessCrashed(e.to_string()));
                    }
                }
            }
        })
        .await;

        if search_res.is_err() {
            let _ = self.child.kill().await;
            return Err(EngineHostError::Timeout { phase: "search" });
        }
        search_res.unwrap()?;

        // Verify `bestmove` legality against canonical `xiangqi-core`!
        if let Some(ref bm_str) = bestmove_uci {
            let mv =
                Move::from_uci_or_iccs(bm_str).map_err(|e| EngineHostError::IllegalEngineMove {
                    move_uci: bm_str.clone(),
                    reason: e.to_string(),
                })?;
            let mut verify = root_state.clone();
            verify
                .apply_move(mv)
                .map_err(|e| EngineHostError::IllegalEngineMove {
                    move_uci: bm_str.clone(),
                    reason: e.to_string(),
                })?;
        }

        Ok(EvaluationManifest {
            search_id: req.search_id.clone(),
            state_id: req.state_id.clone(),
            engine_id: req.engine_id.clone(),
            reported_engine_name: self.reported_name.clone(),
            binary_sha256: req.binary_sha256.clone(),
            network_sha256: req.network_sha256.clone(),
            rules_profile: req.rules_profile.clone(),
            effective_config_hash: req.effective_config_hash(),
            threads: req.threads,
            hash_mb: req.hash_mb,
            multipv: req.multipv,
            budget: req.budget.clone(),
            cancelled,
            elapsed_ms: started.elapsed().as_millis() as u64,
            bestmove_uci,
            infos,
        })
    }

    fn process_line(
        line: &str,
        req: &SearchRequest,
        root_state: &GameState,
        infos: &mut Vec<NormalizedInfoLine>,
    ) -> Result<Option<Option<String>>, EngineHostError> {
        if let Some(info) = parse_uci_info_line(line, &req.search_id, &req.state_id, root_state) {
            infos.push(info);
            return Ok(None);
        }
        if let Some(bm) = parse_uci_bestmove_line(line) {
            return Ok(Some(bm));
        }
        Ok(None)
    }

    fn supports_option(&self, name: &str) -> bool {
        let prefix = format!("option name {name}");
        self.discovered_options
            .iter()
            .any(|o| o.starts_with(&prefix))
    }

    pub async fn shutdown(mut self) {
        let _ = self.send_line("quit").await;
        if timeout(Duration::from_millis(500), self.child.wait())
            .await
            .is_err()
        {
            let _ = self.child.kill().await;
        }
    }
}

pub fn crate_ready() -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use xiangqi_core::STARTING_FEN;

    #[test]
    fn normalizes_perspective_and_strips_illegal_pv_tail() {
        let mut black_turn = GameState::from_fen(STARTING_FEN, ADOPTED_RULES_PROFILE).unwrap();
        black_turn
            .apply_move(Move::from_uci_or_iccs("h2e2").unwrap())
            .unwrap();
        // Black is to move; UCI `score cp 35` means +35 for Black -> -35 from Red's perspective!
        let info = parse_uci_info_line(
            "info depth 8 seldepth 12 multipv 1 score cp 35 wdl 410 490 100 nodes 24000 nps 800000 time 30 pv h9g7 h0g2 a0a9",
            "s-1",
            "st-1",
            &black_turn,
        )
        .unwrap();
        assert_eq!(info.score_red_perspective, -35);
        assert_eq!(info.wdl, Some([410, 490, 100]));
        // `h9g7` and `h0g2` are legal; `a0a9` is blocked by pawns/pieces so PV truncates cleanly!
        assert_eq!(info.validated_pv_uci, vec!["h9g7", "h0g2"]);

        // Standalone bestmove without preceding info
        assert_eq!(
            parse_uci_bestmove_line("bestmove h9g7 ponder h0g2"),
            Some(Some("h9g7".to_string()))
        );
        assert_eq!(parse_uci_bestmove_line("bestmove (none)"), Some(None));
    }

    #[tokio::test]
    async fn handles_fragmented_bytes_across_line_reader() {
        let (mut writer, reader) = tokio::io::duplex(256);
        tokio::spawn(async move {
            writer.write_all(b"info depth 2 score ").await.unwrap();
            writer
                .write_all(b"cp 18 nodes 120 pv h2e2\nbestmo")
                .await
                .unwrap();
            writer.write_all(b"ve h2e2\n").await.unwrap();
        });
        let root = GameState::default();
        let mut lines = BufReader::new(reader).lines();
        let l1 = lines.next_line().await.unwrap().unwrap();
        let l2 = lines.next_line().await.unwrap().unwrap();
        let info = parse_uci_info_line(&l1, "s2", "st2", &root).unwrap();
        assert_eq!(info.validated_pv_uci, vec!["h2e2"]);
        assert_eq!(parse_uci_bestmove_line(&l2), Some(Some("h2e2".to_string())));
    }
}
