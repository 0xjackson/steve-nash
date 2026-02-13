# GTO Poker Solver — Full Implementation Plan

## Vision

A complete GTO poker solver built in Rust. Pre-solves all streets (preflop through river) for every strategically distinct flop, every position pair, and both SRP and 3-bet pot types. Queries return **instantly** (<100ms) from cached solutions. Pre-solve runs locally in the background — each spot becomes usable the moment it finishes. Designed for **live online play** (Club GG and similar) with a local web UI for fast input.

**Primary interface** — localhost web UI:
- Visual card grid (click hole cards + board cards)
- Position buttons (UTG/HJ/CO/BTN/SB/BB)
- Action buttons (Check/Bet 33%/Bet 66%/Bet 100%/Raise/Call/Fold)
- Preflop action tracking (who opened, who called, who 3-bet)
- Instant strategy display: action, bb amount, frequency, reasoning
- All reads from pre-solved cache — zero latency

**CLI shorthand** — for quick lookups:
```bash
gto AhKs BTN                    # preflop
gto AhKs BTN Ks9d4c             # flop (SRP assumed)
gto AhKs BTN Ks9d4c xb          # flop, villain checked then bet — what do I do?
gto AhKs BTN Ks9d4c7h           # turn
```

**Key constraint**: All solvers are **2-player (heads-up)** — this is a fundamental limitation of CFR-based game theory. True multiway GTO is computationally intractable. For multiway pots, the system picks the primary villain (most relevant opponent) and applies a tightening heuristic.

---

## Phase Overview

| Phase | What | Status | Solve Time | Query Time |
|---|---|---|---|---|
| 1 | Push/fold solver | DONE | 3-5 sec | Instant |
| 2 | Full preflop (open/3bet/4bet/5bet) | DONE | 3 min | Instant |
| 3 | River solver | DONE | 1-5 sec/spot | Instant |
| 4 | Turn solver (includes river) | DONE | 15-45 sec/spot | Instant |
| 5 | Flop solver (includes turn+river) | DONE | 1-4 min/spot | Instant |
| 6 | CFR engine optimization | DONE | N/A | N/A |
| 7 | Local batch pre-solve + query interface | DONE | Background | <1 sec |
| 8 | Strategy engine + play mode (v1) | DONE | N/A | <1 sec |
| 9 | Critical fixes (cache, data, navigation) | DONE | N/A | N/A |
| 10 | Full batch expansion (1,755 flops + 3BP) | DONE | ~54 days | Instant |
| 11 | Play mode rewrite + better output | TODO | N/A | N/A |
| 12 | Local web UI | TODO | N/A | Instant |

---

## Architecture

### How It Works

```
PRE-SOLVE (local, one-time background)          USE (every hand, instant)
┌──────────────────────────────┐                ┌─────────────────────────┐
│ MCCFR solver grinds through  │                │ Web UI / CLI query      │
│ all flop × position × pot    │                │         |               │
│ type combinations            │                │    Load from disk       │
│         |                    │                │         |               │
│    Save to bincode cache     │                │  Navigate game tree     │
│    (flop + turn + river      │                │  (action history)       │
│     strategies together)     │                │         |               │
│         |                    │                │  Return answer <100ms   │
│ Each spot: 1-4 min           │                └─────────────────────────┘
│ ~54 days for full coverage   │
│ Resumable, runs in background│
└──────────────────────────────┘
```

### Spot-Based Model

Every scenario is a **spot**: a specific game state defined by:
- Who is IP / OOP (position pair, e.g., BTN vs BB)
- What ranges each player has (derived from preflop solver)
- Board cards (3 for flop — turn/river strategies embedded)
- Pot size and effective stack (determines pot type: SRP or 3BP)
- Allowed bet sizes

Each spot is solved independently. This is the same approach PioSolver uses.

### Pre-Solve Coverage

| Pot Type | Flops | Position Pairs | Total Spots | Solve Time (local) | Storage |
|---|---|---|---|---|---|
| SRP (6bb pot, 97bb stack) | 1,755 | 9 | 15,795 | ~27 days | ~8 GB |
| 3BP (20bb pot, 80bb stack) | 1,755 | 9 | 15,795 | ~27 days | ~8 GB |
| **Total** | **1,755** | **9** | **31,590** | **~54 days** | **~16 GB** |

Each flop solution includes embedded turn and river template strategies — no separate turn/river solves needed. Every street is instant from one flop solve.

**Position pairs** (in priority order):
1. BTN vs BB, CO vs BB, HJ vs BB, UTG vs BB, SB vs BB
2. BTN vs SB, CO vs BTN, HJ vs BTN, UTG vs BTN

---

## Phases 1–6: Core Solvers (DONE)

### Phase 1: Push/Fold Solver — DONE

SB shoves or folds, BB calls or folds. Nash equilibrium for any stack depth.

**Files**: `src/game_tree.rs`, `src/cfr.rs`
**Tests**: 13 integration tests in `tests/test_solver.rs`

### Phase 2: Full Preflop Solver — DONE

5-node game tree: open -> 3-bet -> 4-bet -> 5-bet -> call/fold. 15 spots for 6-max.

**Files**: `src/preflop_solver.rs`, CLI in `src/cli.rs`
**Tests**: 15 integration tests in `tests/test_preflop_solver.rs`, 7 unit tests
**Commands**: `gto solve preflop`, `gto range --solved`

**Key decisions**:
- Spot-based 2-player decomposition
- Single raise sizes (2.5bb open, 3x 3-bet, 2.5x 4-bet)
- IP uses postflop action order (BTN last = most IP)
- Equity realization: IP = raw equity, OOP = equity x 0.95
- Disk cache at `~/.gto-cli/solver/`

### Phase 3: River Solver — DONE

Heads-up river CFR+ solver with exact showdown evaluation at the combo level.

**Files**: `src/river_solver.rs`, `src/postflop_tree.rs`, CLI in `src/cli.rs`
**Tests**: 17 tests in `tests/test_river_solver.rs` + 5 unit tests in `src/postflop_tree.rs`
**Commands**: `gto solve river --board --oop --ip --pot --stack --iterations`

**Key decisions**:
- Exact combo-level computation (not bucketed)
- Blocker-aware showdown precomputation via `evaluate_fast()`
- Alternating-traverser CFR+ with `opp_reach` vector propagation
- Exploitability via best-response traversal
- Cache to `~/.gto-cli/solver/river_{board}_{pot}_{stack}.bin`

### Phase 4: Turn Solver — DONE

Heads-up turn+river CFR+ solver with chance nodes for river card dealing and flat-array storage.

**Files**: `src/flat_cfr.rs`, `src/turn_solver.rs`, `src/postflop_tree.rs`, `src/cli.rs`
**Tests**: 9 integration tests, 9 unit tests in `src/flat_cfr.rs`, 5 tree tests

**Key decisions**:
- `FlatCfr` engine with contiguous f32 arrays (~5x memory reduction vs HashMap)
- Node-based layout: `offsets[node] + hand * num_actions + action`
- Two `FlatCfr` instances (one per player) — no snapshot needed
- Full river enumeration (~48 cards), hand strengths re-evaluated per river card
- Turn bet sizes: 50%, 100% pot. River bet sizes: 33%, 67%, 100% pot

### Phase 5: Flop Solver — DONE

External Sampling MCCFR with template trees and equity-based hand bucketing.

**Files**: `src/bucketing.rs`, `src/flop_solver.rs`, `src/cli.rs`, `src/lib.rs`, `src/main.rs`
**Tests**: 10 integration tests, 6 unit tests in `src/bucketing.rs`

**Key decisions**:
- **Template trees**: 3 separate single-street action trees (flop, turn template, river template)
- **6 FlatCfr instances**: 1 per player x 3 streets, total ~1 MB memory
- **Equity-based hand bucketing**: ~200 buckets per street via equal-frequency binning
- **External sampling MCCFR**: each iteration samples one turn + one river card
- **Bet sizes**: Flop 33%/75%, Turn 66%, River 50%/100%
- Cache to `~/.gto-cli/solver/flop_{board}_{pot}_{stack}.bin`

**Critical finding (Phase 9 fix)**: The flop solver trains turn and river FlatCfr instances during MCCFR but `extract_solution` only saves flop-level strategies. Turn and river data is computed then **thrown away**. Phase 9 fixes this.

### Phase 6: CFR Engine Optimization — DONE

- Rayon parallel hand traversal for flop solver
- Regret pruning in CFR traversal functions
- Precomputed bucket and score lookup tables
- Bincode serialization (5-10x smaller than JSON, faster load)

---

## Phase 7: Local Batch Pre-Solve — DONE

*Adapted from original Railway cloud deployment plan to local-only solving.*

**Files**: `src/batch.rs`, `src/cli.rs`
**Commands**: `gto solve batch [--stack 100] [--srp-only] [--iterations 500000] [--limit N]`

**What was built**:
- Batch solve orchestrator with resumability (skips existing cache files)
- 50 representative flop boards covering major textures (high dry, broadway, connected, low, monotone, paired, wheel)
- 9 position pairs in priority order
- SRP pot type with optional 3-bet pots
- Progress output: `[47/450] Solving AsKd7c BTN vs BB (SRP) ... done (1.2 min)`
- Range derivation from preflop solver (>5% threshold)

**Known issues** (fixed in Phase 9):
- Cache key missing position pair — solutions overwrite each other
- Only 50 hardcoded flops instead of all 1,755
- Turn/river data not saved from flop solve

---

## Phase 8: Strategy Engine + Play Mode (v1) — DONE

**Files**: `src/strategy.rs`, `src/play.rs`, `src/cli.rs`, `src/main.rs`

**What was built**:
- `StrategyEngine`: loads preflop solution, derives postflop ranges, routes queries to correct solver (flop/turn/river), cache-or-solve-on-demand logic
- `gto query AhKs BTN [Ks9d4c]`: one-shot query command
- Shorthand detection: `gto AhKs BTN Ks9d4c` auto-prepends "query"
- `gto play`: interactive mode (hand -> position -> board -> advice per street)
- `PotType` enum (SRP/3BP/4BP) with standard pot/stack values
- Default villain mapping (BTN->BB, CO->BB, etc.)
- Combo lookup with both orderings (AhKs and KsAh)
- Pretty display with unicode suit symbols
- Heuristic fallback when solver unavailable

**Known issues** (fixed in Phase 9 and 11):
- **Root-node only**: `lookup_in_flop_solution` always returns the first matching node (root). No action sequence navigation — if villain bets, raises, or checks, you still get the same answer.
- **No hero/villain action separation**: Play mode asks one "What happened?" prompt instead of tracking hero and villain actions separately.
- **Always assumes SRP**: Hardcoded pot=6, stack=97. No 3-bet or 4-bet pot detection.
- **No multiway support**: Always assumes HU (2 players).
- **Raw frequency output**: Shows "CHECK (45%), BET 33% (30%)" without bb amounts or clear recommendation.

---

## Phase 9: Critical Fixes — TODO

*Must be completed before any batch runs, or batch output is wasted.*

### 9.1: Fix cache key to include position pair

**Problem**: Cache path is `flop_{board}_{pot}_{stack}.bin`. BTN vs BB and CO vs BB on the same board have different ranges but the same cache key. Whichever solves last overwrites the other — **8 of 9 position pair solutions are lost**.

**Fix**: Change cache path to `flop_{board}_{pot}_{stack}_{oop_pos}_{ip_pos}.bin`. Apply to all three solver caches (flop, turn, river).

**Files**: `src/flop_solver.rs` (cache_path, load_cache), `src/turn_solver.rs`, `src/river_solver.rs`, `src/strategy.rs` (load calls)

**Impact**: Accuracy — critical. Without this, batch pre-solve is wasted.
**Compute**: Zero. **Effort**: ~30 min.

### 9.2: Save turn/river template strategies from flop solve

**Problem**: The flop solver trains 6 FlatCfr instances (flop/turn/river x OOP/IP) during MCCFR. `extract_solution()` only saves flop-level strategies. The turn and river FlatCfr data is **computed then discarded**.

When a user later queries the turn or river, the system runs a **separate** turn/river solver from scratch (15-45s for turn, 1-5s for river) — recomputing data that already existed.

**Fix**:
1. Add turn/river FlatCfr data to `FlopSolution` struct (the 4 template FlatCfr instances: turn_oop, turn_ip, river_oop, river_ip)
2. Add turn/river bucket mappings to `FlopSolution` (combo -> bucket for each sampled turn/river card)
3. Modify `extract_solution()` to serialize the template FlatCfr data
4. Add `query_turn_from_flop()` and `query_river_from_flop()` to `StrategyEngine`
5. Modify `query_turn()` and `query_river()` to check flop solution first before running a separate solver

**Accuracy**: Bucket-level (~200 buckets) — same accuracy as the flop strategy itself. Not as precise as a dedicated combo-level turn/river solve, but ~95%+ same answer and instant.

**Cache size**: ~200KB -> ~500KB per spot. Total for full batch: ~8 GB (SRP) or ~16 GB (SRP + 3BP).

**Compute**: Zero extra solve time — data is already computed.
**Effort**: ~2-3 hours.

### 9.3: Action sequence navigation in strategy lookups

**Problem**: `lookup_in_flop_solution()` (strategy.rs:372-374) iterates `solution.strategies` and returns the **first** `FlopNodeStrategy` matching hero's side. This is always the root node — what to do when first to act.

If villain bets into you, raises your bet, or checks (and you need to decide facing a check), the system returns the **wrong answer** — it always says what to do at the start of the street.

The data IS there: `FlopSolution.strategies` contains `FlopNodeStrategy` entries for every action node in the tree, each with a `node_id`. The postflop tree has a defined structure: root -> check/bet -> call/raise/fold -> etc.

**Fix**:
1. Add `action_path: Vec<String>` parameter to lookup functions
2. Map action strings ("check", "bet_33", "bet_75", "raise", "call") to tree edges
3. Traverse from root following the action path to find the correct `node_id`
4. Return the strategy at that node
5. Add node-to-parent and node-to-action mappings in `FlopNodeStrategy` or a separate tree index structure

**Example**: Hero is OOP on flop. Hero checks. Villain bets 33% pot. What should hero do?
- Action path: `["check", "bet_33"]` (OOP checked, IP bet 33%)
- Navigate tree: root (OOP acts) -> check action -> IP decision node -> bet 33% action -> OOP facing bet node
- Return OOP's strategy at that node: `CALL 55%, RAISE 30%, FOLD 15%`

**Impact**: Accuracy — massive. Goes from "correct 1 out of ~5 decision points per street" to "correct at every decision point."
**Compute**: Zero (tree traversal, not solving).
**Effort**: ~3-4 hours.

---

## Phase 10: Full Batch Expansion — DONE

*Flop enumerator generates all 1,755 canonical flops. Batch supports `--all-flops` flag, 3-bet pots via `--srp-only=false`, and board-first iteration with strategic priority sorting.*

### 10.1: Generate all 1,755 strategically distinct flops

**Problem**: Batch has 50 hardcoded representative flops. There are 22,100 possible 3-card flops (52C3), but many are strategically equivalent due to suit isomorphism. E.g., Ks9d4c and Kd9s4h play identically (same ranks, same suit pattern). After removing suit isomorphisms, there are **1,755** distinct flops.

**Fix**: Write an algorithmic flop enumerator that generates all 1,755 canonical flops using suit isomorphism reduction.

**Coverage**: 50 flops (~60%) -> 1,755 flops (100%).
**Compute**: 450 spots -> 15,795 spots (SRP). Solve time: ~27 days.
**Effort**: ~2 hours.

### 10.2: Add 3-bet pot support

**Problem**: 3-bet pots are ~20-30% of hands played. SPR ~4 (3BP) vs SPR ~16 (SRP) means completely different strategy. Overpairs go all-in in 3BP but play cautiously in SRP. Currently every 3-bet pot answer uses SRP parameters and is **wrong**.

**Fix**: Run batch with `srp_only: false`. Solves each flop for both:
- SRP: 6bb pot, 97bb stack (SPR ~16)
- 3BP: 20bb pot, 80bb stack (SPR ~4)

**Compute**: Doubles the batch. 15,795 -> 31,590 spots. ~54 days total.
**Disk**: ~16 GB.
**Effort**: ~1 hour (batch already supports `srp_only: false`, just needs the flag).

### 10.3: Optimize batch iteration order

**Problem**: Current batch iterates position pairs as outer loop, boards as inner. After 12 hours (~288 spots), only BTN vs BB is covered with ~288 boards. Zero coverage for CO vs BB, HJ vs BB, etc.

**Fix**: Interleave: for each board, solve all position pairs before moving to the next board. This way the most common boards get full position coverage first.

**Priority ordering**:
1. High-card boards (A-high, K-high) — most common in real play
2. Broadway connected boards (KQT, JTs) — frequent, complex
3. Paired boards — common, unique strategy
4. Medium connected boards (987, 864)
5. Low boards, monotone, wheel boards
6. Everything else

**Impact**: After 12 hours, ~32 boards fully covered across all 9 positions instead of 288 boards for one position.
**Effort**: ~1 hour.

### 10.4: Pre-solve resource requirements

| Resource | Amount | Notes |
|---|---|---|
| **Disk** | ~16 GB | 31,590 spots x ~500 KB each |
| **RAM** | ~1.5 GB | One spot at a time, memory recycled |
| **CPU** | 100% all cores | Rayon parallelizes within each solve |
| **Time** | ~54 days | 27 days SRP + 27 days 3BP |
| **Resumability** | Yes | Skips already-cached spots on restart |

Mac remains usable during batch — the solver uses ~1.5 GB RAM and runs in background. Can be stopped and resumed at any time.

---

## Phase 11: Play Mode Rewrite + Better Output — TODO

### 11.1: Proper action tracking (hero vs villain)

**Problem**: Current play mode asks one generic "What happened?" prompt after showing advice. It doesn't know WHO acted or WHAT they did. Can't navigate the game tree without this.

**Fix**: Complete rewrite of the postflop action loop:
1. Determine who acts first (OOP player)
2. Show advice for the acting player
3. Ask "Your action?" (if hero acts) or "Villain action?" (if villain acts)
4. Record the action, update pot/stack
5. Navigate to the next tree node
6. Show advice for the next decision point
7. Repeat until end of street or fold

**Example flow** (hero is IP):
```
Board: Ks 9d 4c  |  Pot: 6bb  |  Stack: 97bb
Villain (BB) acts first...
Villain? check
  -> Your turn (BTN, IP):
  -> BET 2bb (72%), CHECK (28%)
Your action? bet 2
Villain? call
  Pot: 10bb  |  Stack: 95bb

Turn: 7h
Villain? bet 7
  -> Facing bet (70% pot):
  -> CALL (55%), RAISE to 22bb (25%), FOLD (20%)
```

### 11.2: Preflop action sequence tracking

**Problem**: Play mode always assumes SRP (6bb pot, 97bb stack). No way to indicate the pot was 3-bet or 4-bet, and no way to specify who opened or who called.

**Fix**:
1. After position, ask: "Preflop action? (open/call/3bet/4bet)" or detect from solver
2. Track: who opened, any callers, any 3-bettors
3. Auto-determine pot type: SRP (open + call), 3BP (open + 3bet + call), 4BP (open + 3bet + 4bet + call)
4. Set pot/stack accordingly: SRP=6/97, 3BP=20/80, 4BP=44/56
5. Determine villain (the aggressor or the caller, depending on hero's position)

### 11.3: Multiway pot handling

**Problem**: Real games have 3-4 players seeing the flop. The solver is 2-player only.

**Fix**:
1. Ask "How many players to flop?" (default 2)
2. If multiway (>2), pick primary villain: the player with position on hero, or the preflop aggressor
3. Solve HU between hero and primary villain
4. Apply tightening heuristic: multiply bet/raise thresholds by ~0.7 for 3-way, ~0.5 for 4-way
5. Display note: "Multiway (3 players) — tighten ranges, play more cautiously"

This is the standard approach — PioSolver and all other GTO tools are also 2-player only.

### 11.4: Better strategy output

**Problem**: Raw frequencies like "CHECK (45%), BET 33% (30%)" aren't useful for live play. Need actual bb amounts and a clear recommendation.

**Fix**:
1. Show bb amounts: "BET 2bb (33% pot, 45%)" instead of "BET 33% (45%)"
2. Show clear recommendation: "Lean CHECK here" (for the highest-frequency action)
3. Show brief reasoning when available: hand strength vs board texture
4. Show equity when calculated: "Your equity: 62% vs villain's range"

**Example output**:
```
-> CHECK (45%) | BET 2bb, 33% pot (30%) | BET 5bb, 75% pot (25%)
   Lean: CHECK — medium-strength hand, control the pot
   Equity: 58% vs BB defend range
```

### 11.5: Enhanced one-liner shorthand with action history

**Fix**: Extend the `gto query` shorthand to support action sequences after the board:
```bash
gto AhKs BTN Ks9d4c xb       # OOP checked (x), IP bet (b) — what does hero do?
gto AhKs BTN Ks9d4c xbc       # OOP checked, IP bet, OOP called — (next street)
gto AhKs BTN Ks9d4c7h xbcx    # turn: OOP checked after action on flop
```

Action codes: `x` = check, `b` = bet, `c` = call, `r` = raise, `f` = fold.

---

## Phase 12: Local Web UI — TODO

### 12.1: Localhost web app

A simple web server (Rust `axum` or similar) serving a local page at `localhost:3000`. Designed for speed — click, don't type.

**Layout**:
```
┌─────────────────────────────────────────────────┐
│  GTO Advisor              [SRP] [3BP] [4BP]     │
├─────────────────────────────────────────────────┤
│  Your cards: [card grid — click 2]              │
│  Position:   [UTG][HJ][CO][BTN][SB][BB]         │
│  Preflop:    [Open][Call][3Bet][4Bet]            │
│  Players:    [2][3][4]                           │
├─────────────────────────────────────────────────┤
│  -> RAISE 2.5bb (92%) | FOLD (8%)               │
├─────────────────────────────────────────────────┤
│  Board: [card grid — click 3/1/1]               │
│  Villain: [Check][Bet 33%][Bet 66%][Bet 100%]   │
├─────────────────────────────────────────────────┤
│  -> CALL 4bb (55%) | RAISE 12bb (25%) | FOLD    │
│     Lean: CALL — top pair good kicker, pot ctrl  │
│     Equity: 62% vs villain range                 │
└─────────────────────────────────────────────────┘
```

**Tech stack**:
- Backend: Rust `axum` web server, reads from same `~/.gto-cli/solver/` cache
- Frontend: Single HTML page with vanilla JS (no framework needed)
- API: `/api/query` endpoint accepting hand, position, board, actions, pot type
- Served on `localhost:3000` — no network, no auth, instant

**Input speed**: 3-5 seconds per decision (click cards + click action). Fast enough for online play with 15-30 second time banks.

### 12.2: Future — screen reader overlay (stretch goal)

Read the poker client window directly (OCR or pixel detection). Auto-detect cards, position, board, pot size. Show advice in an overlay. Game-specific parsing for Club GG.

This is a much larger project and not needed for v1.

---

## Performance Summary

### Solve Times (one-time, background)

| Street | Per Spot | Iterations | Memory | Accuracy |
|---|---|---|---|---|
| Preflop | 3-5 sec | 50K | <1 MB | <0.15 bb exploit |
| River | 1-5 sec | 10K | <10 MB | <0.1% pot |
| Turn | 15-45 sec | 50K | 50-200 MB | <0.5% pot |
| Flop | 1-4 min | 500K-1M MCCFR | 0.5-1.5 GB | <1% pot |

### Query Times (user experience, from cache)

**Every query: <100ms.** Flop, turn, and river all served from the cached flop solution. No separate turn/river solves needed after Phase 9.

### Total Pre-Solve for Full Coverage

- **Time**: ~54 days running in background (27 SRP + 27 3BP)
- **Storage**: ~16 GB on disk
- **RAM**: ~1.5 GB during solving
- **Result**: Instant GTO answers for 100% of flop textures, all position pairs, SRP + 3-bet pots, all streets

---

## Key Decisions Summary

| Decision | Choice | Rationale |
|---|---|---|
| Solver algorithm | 2-player CFR+/MCCFR | Multiway GTO is intractable; HU with heuristic adjustment is industry standard |
| CFR storage (postflop) | Flat f32 arrays (`FlatCfr`) | 10x less memory than HashMap + f64 |
| Flop algorithm | External sampling MCCFR | Too many runouts for full traversal |
| Hand abstraction | OCHS bucketing, ~200 per street | Needed for flop memory budget |
| Turn/river from flop | Save template FlatCfr in flop solution | Zero extra solve time, instant multi-street queries |
| Bet sizes | 33/66/100% pot (configurable) | Balance accuracy vs tree size |
| Pre-solve scope | All 1,755 flops x 9 positions x 2 pot types | 100% coverage, ~54 days local compute |
| Cache format | bincode | 5-10x smaller than JSON, fast load |
| Cache key | board + pot + stack + oop_pos + ip_pos | Prevents position pair collisions |
| Range derivation | From preflop solver (>5% threshold) | Automatic, no manual range entry |
| Multiway handling | HU solve vs primary villain + tightening | Only feasible approach given 2-player solver limitation |
| Primary UX | Localhost web UI (click-based) | Fast enough for live online play (3-5 sec per decision) |
| Secondary UX | CLI shorthand with action codes | For quick lookups and study |
| Cache portability | `~/.gto-cli/solver/` directory | Copy to any machine, share, back up |

---

## Design Session Notes

### 2025-02-12: Full audit and roadmap discussion

**Context**: Phases 1-8 implemented. Evaluated readiness for real-world use (live online poker, Club GG).

**Critical issues found**:

1. **Cache key collision** — flop/turn/river cache keys don't include position pair. BTN vs BB and CO vs BB overwrite each other. 8 of 9 position pair solutions lost from batch.

2. **Turn/river data discarded** — flop MCCFR trains 6 FlatCfr instances but only saves flop strategies. Turn/river data thrown away, forcing separate on-demand solves (15-45s for turn, 1-5s for river).

3. **Root-node only lookups** — strategy lookups always return the first tree node (root). No action sequence navigation. Advice is only correct for the first decision on each street. Facing a bet, raise, or check gets the wrong answer.

4. **HU-only assumption** — entire system assumes 2 players. No tracking of preflop action sequence (who opened, who called, who 3-bet). No multiway support.

5. **SRP-only** — hardcoded pot=6, stack=97. 3-bet pots (~20-30% of hands) get wrong answers.

6. **UX too slow for live play** — typing full hand histories in CLI while clock is ticking doesn't work. Need click-based input (web UI).

**Architecture decisions**:
- Multiway: HU solve + tightening heuristic (industry standard, computationally necessary)
- Turn/river: embed in flop solution (bucket-level, ~95% accuracy, zero extra compute)
- Batch: all 1,755 flops + 3BP = 31,590 spots, ~54 days, ~16 GB
- UI: localhost web app with card grid + action buttons

**Compute analysis**:
- Railway cloud rejected: Apple Silicon faster per-core than shared cloud vCPUs. ~2x speedup for ~$200 cost. Not worth it.
- Local batch: ~54 days, zero cost, Mac remains usable (~1.5 GB RAM).
- Spots become usable as they finish — can play immediately, coverage grows daily.
