# GTO Solver Upgrade Plan

## Current State (Post-Phase 0)

- 5,200+ lines of source code across 16 modules
- Fast hand evaluator: histogram + bitmask approach (~10-50M evals/sec)
- Equity uses Monte Carlo with fast evaluator (7x speedup achieved)
- Strategy is heuristic-based (hardcoded rules, not computed equilibria)
- No rake support
- 333 passing tests

## Target State

A working CFR+ solver that computes Nash equilibrium strategies for
Hold'em spots, backed by a fast lookup-table hand evaluator. Integrated
into the existing CLI with rake support.

---

## Phase 0: Fast Hand Evaluator -- COMPLETE

**Goal**: Replace the old itertools evaluator (~500K evals/sec) with a
fast evaluator for the solver's inner loops.

**Approach taken**: Built a zero-allocation evaluator using rank
histograms, 13-bit suit masks, and a precomputed 8KB straight-detection
table. No `build.rs` or 130MB lookup table needed. The evaluator works
for 5, 6, or 7 cards and returns a packed u32 score for direct
comparison.

**Results**:
- Equity simulations: **7x faster** (audit tests: 11.8s -> 1.6s)
- 20 unit tests + cross-validation against old evaluator: 100% agreement
- All 333 tests pass

### Steps

- [x] **0.1** Created `src/card_encoding.rs` -- Card <-> u8 index (0-51) mapping
- [x] **0.2** Created `src/lookup_eval.rs` -- fast evaluator with STRAIGHT_TABLE
- [x] **0.3** `evaluate_fast(&[u8]) -> u32` handles 5/6/7 cards
- [x] **0.4** 20 unit tests + cross-validation
- [x] **0.5** Wired into `equity.rs` -- Monte Carlo now uses fast path

**Optimization note for Phase 3**: Current speed (10-50M evals/sec) is
sufficient for Phase 1-2. Before Phase 3 (postflop solver), we can gain
another 2-4x by:
- Removing remaining Vec allocations in hot paths
- Adding `#[inline(always)]` on inner functions
- Ensuring `hand_evaluator.rs` wrapper delegates to `evaluate_fast`
  instead of using itertools for any code paths
- If needed: switch to TwoPlusTwo 130MB table for 200M+/sec

**New files**: `src/lookup_eval.rs`, `src/card_encoding.rs`
**Modified files**: `src/equity.rs`, `src/lib.rs`, `src/main.rs`

---

## Phase 1: Push/Fold Solver

**Goal**: Solve the simplest poker decision -- shove all-in or fold.
Used in short-stack tournament play. This builds and validates the core
CFR+ infrastructure.

**How it works**: Two players, each dealt a hand. Player 1 can shove
(bet all chips) or fold. If shove, Player 2 can call or fold. That's
the entire game tree -- just 2 decision points. CFR+ iterates over this
tree thousands of times, and for each hand, tracks how much "regret" it
has for not picking the other action. Over time, the strategy converges
to Nash equilibrium -- the mathematically unexploitable answer.

### Steps

- [ ] **1.1** Create `src/cfr.rs` -- the core CFR+ algorithm
  - Information set representation (player's hand + game state)
  - Regret tracking per information set per action
  - Strategy computation from cumulative regret (regret matching)
  - CFR+ specific: floor negative regrets to zero

- [ ] **1.2** Create `src/game_tree.rs` -- push/fold tree
  - Node types: Terminal (showdown/fold), Action (decision point)
  - Push/fold tree: root -> P1 shoves or folds -> P2 calls or folds
  - Payoff calculation at terminal nodes (using lookup evaluator)

- [ ] **1.3** Implement the push/fold solver loop
  - Deal all possible hand combinations to both players
  - Run CFR+ iterations (target: 10,000-100,000 iterations)
  - Track convergence (exploitability decreasing over iterations)

- [ ] **1.4** Output push/fold charts
  - For each stack depth (5bb, 10bb, 15bb, 20bb), produce:
    - Push range for Player 1 (SB)
    - Call range for Player 2 (BB)
  - Display as familiar 13x13 grid

- [ ] **1.5** Add `gto solve pushfold` CLI command
  - `gto solve pushfold --stack 10 --rake 0`
  - Shows solved push/fold ranges
  - Optional: `--ante` for tournament antes

- [ ] **1.6** Test and validate
  - Compare against known push/fold charts (Jennings-Sklansky)
  - Verify exploitability is near zero
  - Test that adding rake tightens ranges appropriately

**New files**: `src/cfr.rs`, `src/game_tree.rs`
**Modified files**: `src/cli.rs`, `src/lib.rs`

---

## Phase 2: Preflop Solver

**Goal**: Solve the full preflop decision tree -- open raise, 3-bet,
4-bet, call, fold -- for all positions. Replaces the static JSON range
charts with computed GTO ranges.

**How it works**: Same CFR+ algorithm but with a bigger game tree.
Instead of just shove/fold, players can raise to specific sizes.
We use "action abstraction" -- instead of allowing any bet size, we
restrict to common ones (2.5bb open, 3x 3-bet, etc.). The solver
iterates over this tree and produces a strategy for every hand at
every decision point.

### Steps

- [ ] **2.1** Extend `src/game_tree.rs` for preflop actions
  - Multi-player action sequence (UTG -> HJ -> CO -> BTN -> SB -> BB)
  - Action abstraction: fold, call, raise 2.5x, raise 3x, all-in
  - Track pot and stacks through the action sequence

- [ ] **2.2** Hand abstraction for preflop
  - 169 canonical starting hands (AA, AKs, AKo, ..., 22, 32o)
  - Each maps to the appropriate number of combos (6, 4, or 12)
  - Probability weighting in CFR iterations

- [ ] **2.3** Equity realization model
  - When preflop action closes (e.g., open + call), the hand goes
    postflop. We need to estimate the EV without solving every flop.
  - Use precomputed all-in equity as a proxy (good enough for preflop)
  - This avoids needing to solve postflop during preflop solving

- [ ] **2.4** Run the preflop solver
  - Solve each position configuration separately (6-max first)
  - Target: converge within 100K iterations
  - Store solved strategies to disk (JSON or binary)

- [ ] **2.5** Replace static ranges with solved ranges
  - `gto range` now shows solver-computed ranges instead of JSON charts
  - Show mixed strategies: "ATs: Raise 70%, Call 30%"
  - Fallback to static ranges if solver hasn't been run

- [ ] **2.6** Add `gto solve preflop` CLI command
  - `gto solve preflop --table 6max --stack 100bb --rake 5`
  - Solves and caches results
  - Progress indicator during solving

**New files**: `src/abstraction.rs`
**Modified files**: `src/game_tree.rs`, `src/cfr.rs`, `src/preflop.rs`, `src/cli.rs`

---

## Phase 3: Postflop Spot Solver

**Goal**: Given a specific flop (or turn/river), positions, and stack
depth, compute GTO strategy for that exact spot.

**Prerequisite**: Optimize hand evaluator to 100M+ evals/sec before
starting this phase (see Phase 0 optimization note).

**How it works**: The game tree now includes bet/check/raise/fold
decisions across multiple streets with community cards being dealt
between streets.

To manage the tree size we use two types of abstraction:
1. **Action abstraction**: limit bet sizes (33%, 66%, 100%, 150% pot)
2. **Card abstraction**: group similar hands into buckets based on
   equity distributions

### Steps

- [ ] **3.0** Optimize hand evaluator to 100M+/sec (see Phase 0 note)

- [ ] **3.1** Card abstraction / hand bucketing
  - Cluster hands by equity distribution against a random range
  - Use k-means or OCHS (Opponent Cluster Hand Strength)
  - ~200 buckets per street is typical
  - Precompute bucket assignments for common flops

- [ ] **3.2** Extend game tree for postflop
  - Street transitions: flop -> turn card dealt -> turn -> river card dealt -> river
  - Action nodes: check, bet (multiple sizes), raise, fold
  - Chance nodes: dealing turn/river cards
  - Terminal nodes: showdown (evaluate hands) or fold (pot awarded)

- [ ] **3.3** Memory-efficient game tree storage
  - A single flop solve can use 1-8GB RAM depending on abstraction
  - Use compact node representation (indices, not pointers)
  - Strategy storage: f32 instead of f64 for regret/strategy arrays
  - Optional: disk-backed storage for large trees

- [ ] **3.4** Implement postflop CFR+ traversal
  - Chance sampling: sample turn/river cards instead of enumerating all
  - External sampling MCCFR (faster convergence for large trees)
  - Alternating updates (update one player's strategy per iteration)

- [ ] **3.5** Solve a specific spot
  - Input: board, hero position, villain position, pot, stacks, bet tree
  - Output: strategy for every hand at every decision point
  - Display: action frequencies per hand

- [ ] **3.6** Add `gto solve spot` CLI command
  - `gto solve spot --board Ks7d2c --hero BTN --villain BB --pot 6 --stack 97 --rake 5`
  - Solves the flop spot with default bet tree
  - Shows strategy breakdown by hand category

- [ ] **3.7** Test and validate
  - Compare simple spots against known PioSolver outputs
  - Verify strategies converge (exploitability -> 0)
  - Verify bet frequencies sum to 1.0 at each node

**New files**: `src/bucketing.rs`, `src/solver.rs`
**Modified files**: `src/game_tree.rs`, `src/cfr.rs`, `src/cli.rs`

---

## Phase 4: Rake Integration

**Goal**: Add rake as a first-class parameter throughout the entire
system -- from basic math to solved strategies.

**How it works**: Rake takes a percentage of every pot (typically 2-10%,
with a cap). This changes the math everywhere:
- Pot odds: you win less than the full pot
- EV: `equity * (pot + bet) * (1 - rake_pct) - (1 - equity) * bet`
- Solver: terminal node payoffs are reduced by rake
- Result: tighter strategies, fewer marginal calls, less bluffing

### Steps

- [ ] **4.1** Add rake to math engine
  - `pot_odds_raked(pot, bet, rake_pct)` -- adjusted pot odds
  - `ev_raked(equity, pot, bet, rake_pct)` -- rake-adjusted EV
  - Update break-even percentages

- [ ] **4.2** Add rake to solver terminal nodes
  - When a hand goes to showdown, winner gets `pot * (1 - rake_pct)`
  - Rake cap: rake never exceeds X dollars per pot
  - Fold pots: no rake (or rake on flop+ only, configurable)

- [ ] **4.3** Add rake to `gto play` session
  - Ask for rake percentage at setup (default 0)
  - Show rake-adjusted pot odds and EV throughout
  - Show how much rake changes the recommended action

- [ ] **4.4** Add `--rake` flag to all relevant CLI commands
  - `gto odds 100 50 --equity 0.35 --rake 5`
  - `gto solve pushfold --stack 10 --rake 5`
  - Display rake impact: "Without rake: +EV. With 5% rake: -EV."

**Modified files**: `src/math_engine.rs`, `src/game_tree.rs`, `src/play.rs`, `src/cli.rs`

---

## Phase 5: Integration & Polish

**Goal**: Wire everything together. `gto play` uses solver results
when available. Solver results are cached. The experience is seamless.

### Steps

- [ ] **5.1** Solver result caching
  - Save solved strategies to `~/.gto-cli/cache/` or `data/solver/`
  - Key by: positions, stack depth, board (if postflop), rake, bet tree
  - Load cached results instead of re-solving

- [ ] **5.2** Upgrade `gto play` to use solver
  - Preflop: use solved ranges instead of static charts
  - Postflop: if a spot has been solved, show solver strategy
  - Fallback: if not solved, use current heuristic (clearly labeled)
  - Show mixed strategies: "Solver says: Bet 66% pot 55%, Check 45%"

- [ ] **5.3** Add `gto solve` parent command
  - `gto solve pushfold` -- phase 1
  - `gto solve preflop` -- phase 2
  - `gto solve spot` -- phase 3
  - `gto solve status` -- show what's been solved/cached

- [ ] **5.4** Progress and convergence display
  - Show iterations, exploitability, ETA during solving
  - Allow interrupting and resuming solves

- [ ] **5.5** Final test suite
  - Solver-specific tests for each phase
  - Integration tests: CLI -> solver -> output
  - Performance benchmarks: evals/sec, solve time, memory usage

---

## File Plan

| File | Action | Phase | Status |
|------|--------|-------|--------|
| `src/card_encoding.rs` | NEW | 0 | DONE |
| `src/lookup_eval.rs` | NEW | 0 | DONE |
| `src/equity.rs` | MODIFY -- use fast evaluator | 0 | DONE |
| `src/cfr.rs` | NEW | 1 | |
| `src/game_tree.rs` | NEW | 1, 2, 3 | |
| `src/abstraction.rs` | NEW | 2 | |
| `src/bucketing.rs` | NEW | 3 | |
| `src/solver.rs` | NEW | 3 | |
| `src/math_engine.rs` | MODIFY -- add raked variants | 4 | |
| `src/play.rs` | MODIFY -- use solver results | 4, 5 | |
| `src/cli.rs` | MODIFY -- add solve commands | 1, 2, 3, 5 | |
| `src/lib.rs` | MODIFY -- register new modules | 0, 1, 2, 3 | DONE (Phase 0) |
| `tests/test_solver.rs` | NEW | 1, 2, 3 | |

## Dependencies to Add

| Crate | Purpose | Phase |
|-------|---------|-------|
| `indicatif` | Progress bars for solver runs | 1 |

## Performance Targets

| Metric | Before | Current | Target |
|--------|--------|---------|--------|
| Hand evaluation | ~500K/sec | ~10-50M/sec | 100M+ (Phase 3) |
| Equity (50K sims) | ~12 sec | ~1.6 sec | <1 sec |
| Push/fold solve (10bb) | N/A | N/A | <5 sec |
| Preflop solve (6max 100bb) | N/A | N/A | <5 min |
| Flop spot solve | N/A | N/A | <30 min |

## Decision Points (Will Ask Before Proceeding)

1. **Preflop solver output format**: Replace JSON range files vs keep both?
2. **Postflop bet tree**: Which bet sizes to allow? (33/66/100/150% pot?)
3. **Card abstraction method**: OCHS vs EMD vs simpler equity bucketing?
4. **Memory budget**: How much RAM is acceptable for a single solve?
5. **Evaluator upgrade**: TwoPlusTwo table vs optimize current approach for Phase 3?
