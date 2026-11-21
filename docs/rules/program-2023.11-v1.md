# Adopted Online Rules Profile: `program-2023.11-v1`

- **Profile ID**: `program-2023.11-v1`
- **Upstream Basis**: Official-Pikafish community, *Chinese Chess Program Competition Rules* (`中国象棋程序竞赛规则`), revision `2023-11-22` (`https://www.pikafish.com/rule.html`)
- **Implementation**: [`crates/xiangqi-core`](../../crates/xiangqi-core/)
- **Golden Fixture Suite**: [`data/fixtures/rules/pikafish_examples.json`](../../data/fixtures/rules/pikafish_examples.json) and [`data/fixtures/rules/movement_and_adjudication.json`](../../data/fixtures/rules/movement_and_adjudication.json)

## Scope

`program-2023.11-v1` is Jade's explicit online adjudication profile. Game creation, invitations, replay views, and evaluation manifests display this profile name so players know which exact repetition, chasing, and move-limit rules govern a match. Historical archived games retain their recorded result and original rule provenance separately rather than having modern adjudication overwrite historical records.

## Terminal Precedence

After every legal move, `GameState::adjudicate()` evaluates terminal conditions in strict precedence order:

1. **Checkmate (`将死` / `checkmate`)**: Mover delivered check and the opponent has zero legal replies → mover wins.
2. **No-Legal-Move Loss (`困毙` / `no_legal_moves_loss`)**: Mover did not deliver check and the opponent has zero legal replies → mover wins (opponent loses; never a draw in Xiangqi).
3. **Threefold Position Repetition (`三局面循环`)**: The canonical `(piece_placement, side_to_move)` key occurs for the third time in the game:
   - Only the closed interval `[first_occurrence ..= third_occurrence]` (`循环局面序列`) is inspected, treated head-to-tail (`循环局面序列视为头尾相连`).
   - **Violation hierarchy**:
     - `PerpetualCheck = 2` (`长将`): Every move by that side in the cycle delivers check.
     - `PerpetualChase = 1` (`长捉`): **No move by either side anywhere in the cycle delivers check**, and there exists at least one opponent non-General piece that is chased (`捉`) after every move by this side and escapes chase (`逃捉`) on every move by the opponent.
     - `Allowed = 0` (`允许循环`): Neither perpetual check nor perpetual chase.
   - **Chase exclusions (Chapter 3 Section 1 Clause 5)**: A legal capture `(attacker -> target)` is a chase unless:
     1. After executing the capture, `attacker` on `target`'s square can be legally recaptured by the opponent — **except** `Horse`/`Cannon` attacking `Chariot` and `Advisor`/`Elephant` attacking `Chariot`/`Horse`/`Cannon` still count as chases even if protected.
     2. `attacker` is a `General` (`帅`/`将`) or `Soldier` (`兵`/`卒`).
     3. `target` is an uncrossed `Soldier` (`未过河兵/卒`).
     4. `attacker` and `target` share the same `Role` and `attacker` is simultaneously in `target`'s legal capture set on the current board.
   - Higher violation level loses; equal violation levels (`2 vs 2`, `1 vs 1`, `0 vs 0`) result in a draw.
4. **Insufficient Attacking Material (`insufficient_material_draw`)**: Neither side retains any `Chariot`, `Horse`, `Cannon`, or `Soldier` → draw.
5. **120-Step Natural Limit (`自然限着` / `natural_limit_120_draw`)**: `effective_non_capture_plies` reaches `120` → draw. Within a non-capture stretch, each side's checking moves count at most `10` times; any check beyond `10` by one side and the forced check-evasion reply immediately following it do not increment `effective_non_capture_plies`. Captures reset both the step counter and both sides' check counters to `0`. Checkmate, `困毙`, and threefold repetition take precedence if reached on step `120`.

## Published Examples Verification (`例1`–`例39`)

All 39 published examples from Chapter 3 Section 3 of the 22 November 2023 Pikafish specification are verified in `crates/xiangqi-core/tests/pikafish_and_fixtures.rs` (both via UCI coordinates and round-trip Chinese/WXF notation) and exported in [`data/fixtures/rules/pikafish_examples.json`](../../data/fixtures/rules/pikafish_examples.json) for cross-binding verification in WASM, Swift, Kotlin, .NET, and GTK.
