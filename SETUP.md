# GTO Poker Solver — Full Implementation Plan

## Vision

A complete GTO poker solver built in Rust. Pre-solves all streets (preflop through river) for every common scenario. Queries return **instantly** (<1 second) from cached solutions. Pre-solve runs on Railway (cloud) in the background — each spot becomes usable the moment it finishes. Results download to local disk for permanent offline use.

**User experience** (the end goal — interactive mode):
```bash
$ gto play
Stack? 100
Hand? AhQd
Position? BTN
→ RAISE 2.5bb (100%)

# BB calls. CLI auto-tracks: pot=6, stack=97.5

Board? Ks9d4c
→ BET 33% (72%), CHECK (28%)

Action? bet 33
# BB calls. CLI auto-tracks: pot=10, stack=95.5

Board? 7h
→ CHECK (55%), BET 66% (45%)

Action? check
# BB checks. Pot stays 10.

Board? Qc
→ BET 66% (80%), BET 33% (12%), CHECK (8%)

Action? bet 66
# CLI auto-tracks: pot=23.2, stack=88.9

Villain? raise 20
→ CALL (85%), FOLD (15%)

# Next hand — stack carries over
Hand? JcTs
Position? CO
→ RAISE 2.5bb (92%), FOLD (8%)
```

Every query: **instant**. All the heavy math is pre-computed and cached after first solve. You just type your cards, the board as it comes, and what happened. The CLI tracks pot, stack, and position automatically.

Also supports one-shot commands for quick lookups:
```bash
# Positional shorthand (no flags needed)
gto AhQd BTN Ks9d4c7hQc 10

# Or with flags for precision
gto action AhQd --position BTN --board Ks9d4c7hQc --pot 10 --stack 95.5
```

---

## Phase Overview

| Phase | What | Status | Solve Time | Query Time |
|---|---|---|---|---|
| 1 | Push/fold solver | DONE | 3-5 sec | Instant |
| 2 | Full preflop (open/3bet/4bet/5bet) | DONE | 3 min | Instant |
| 3 | River solver | DONE | 1-5 sec/spot | Instant |
| 4 | Turn solver (includes river) | TODO | 15-45 sec/spot | Instant |
| 5 | Flop solver (includes turn+river) | TODO | 1-4 min/spot | Instant |
| 6 | Infrastructure: CFR engine optimization | TODO | N/A | N/A |
| 7 | Railway deployment + batch pre-solve | TODO | Background | Instant |
| 8 | Unified CLI query interface | TODO | N/A | <1 sec |

---

## Architecture

### How It Works

```
PRE-SOLVE (Railway, one-time)                    USE (laptop, every hand)
┌─────────────────────────┐                     ┌──────────────────────┐
│ CFR+ solver grinds      │                     │ gto action AhQh ...  │
│ through game trees      │  download cache     │         │            │
│         │               │ ──────────────────> │    Load from disk    │
│    Save to JSON cache   │   (~20-80 GB)       │         │            │
│         │               │                     │  Look up strategy    │
│ Each spot: 1s - 4min    │                     │         │            │
│ Usable immediately      │                     │  Return answer <1s   │
└─────────────────────────┘                     └──────────────────────┘
```

### Spot-Based Model

Every scenario is a **spot**: a specific game state defined by:
- Who is IP / OOP
- What ranges each player has (from preflop solve)
- Board cards
- Pot size and effective stack
- Allowed bet sizes

Each spot is solved independently. This is the same approach PioSolver uses.

### Pre-Solve Coverage

| Tier | Flops | Scenarios | Total Spots | Solve Time (Railway) | Storage | Coverage |
|---|---|---|---|---|---|---|
| Tier 1 | 50 common | 5 main | 250 | ~6 hours | 2-10 GB | ~60% |
| Tier 2 | 200 common | 15 | 3,000 | ~2 days | 20-80 GB | ~90% |
| Tier 3 | All 1,755 | 30 | 52,650 | ~2 weeks | 500 GB+ | 100% |

**Recommended**: Tier 2. Covers 90% of real play for ~$20-40 in Railway compute. Results stored locally forever.

---

## Phase 1: Push/Fold Solver — DONE

SB shoves or folds, BB calls or folds. Nash equilibrium for any stack depth.

**Files**: `src/game_tree.rs`, `src/cfr.rs`
**Tests**: 13 integration tests in `tests/test_solver.rs`

---

## Phase 2: Full Preflop Solver — DONE

5-node game tree: open → 3-bet → 4-bet → 5-bet → call/fold. 15 spots for 6-max.

**Files**: `src/preflop_solver.rs`, CLI in `src/cli.rs`
**Tests**: 15 integration tests in `tests/test_preflop_solver.rs`, 7 unit tests
**Commands**: `gto solve preflop`, `gto range --solved`

**Key decisions made**:
- Spot-based 2-player decomposition
- Single raise sizes (2.5bb open, 3x 3-bet, 2.5x 4-bet)
- IP uses postflop action order (BTN last = most IP)
- Equity realization: IP = raw equity, OOP = equity × 0.95
- Disk cache at `~/.gto-cli/solver/`

---

## Phase 3: River Solver — DONE

Heads-up river CFR+ solver with exact showdown evaluation at the combo level.

**Files**: `src/river_solver.rs`, `src/postflop_tree.rs`, CLI in `src/cli.rs`
**Tests**: 17 tests in `tests/test_river_solver.rs` + 5 unit tests in `src/postflop_tree.rs`
**Commands**: `gto solve river --board --oop --ip --pot --stack --iterations`

**Key decisions made**:
- Exact combo-level computation (not bucketed) — river is small enough
- Blocker-aware showdown precomputation: evaluate each combo once via `evaluate_fast()`, build validity tables
- Alternating-traverser CFR+ with per-hand sequential tree traversal and `opp_reach` vector propagation
- Reuses existing `CfrTrainer`/`InfoSetKey`/`InfoSetData` from `src/cfr.rs`
- `postflop_tree.rs` is generic — reusable for turn/flop solvers
- Exploitability via best-response traversal
- Cache to `~/.gto-cli/solver/river_{board}_{pot}_{stack}.json`

**Architecture**:
- `src/postflop_tree.rs`: `TreeNode` enum (Action/Terminal), `TreeConfig`, `build_tree()` with configurable bet/raise sizes, max raises, all-in clamping
- `src/river_solver.rs`: `ShowdownTable` (precomputed scores + blocker tables), `solve_river()`, `compute_exploitability()`, strategy extraction, caching, display

### Known Performance Issues (to address before Phase 4)

These are functional but suboptimal. Phase 4 (turn solver) will need these fixed to scale:

1. **Single-threaded CFR iteration**: Each combo's tree traversal is independent but all mutate the shared `CfrTrainer`. Fix: collect regret updates per-combo into thread-local buffers, batch-apply after all combos. Enables rayon parallelization.

2. **opp_reach vector allocation**: A new `Vec<f64>` is heap-allocated at every opponent action node × every action × every iteration. For 1000 combos × 30 nodes × 5 actions × 10K iterations = ~1.5 billion allocations. Fix: pre-allocate a stack of reusable buffers sized to max(num_oop, num_ip), pass by mutable reference through recursion.

3. **Strategy snapshot per iteration**: A fresh `HashMap<u16, Vec<Vec<f64>>>` is built every iteration to snapshot opponent strategies (avoids borrow conflict with trainer). Fix: allocate the snapshot structure once before the iteration loop, overwrite values in-place each iteration instead of rebuilding.

4. **HashMap-based CfrTrainer**: The `HashMap<InfoSetKey, InfoSetData>` with `Vec<f64>` per info set is ~120 bytes per entry + hash overhead. Phase 4 introduces `FlatCfr` with contiguous f32 arrays (~24 bytes per info set, zero overhead). This is the biggest win for turn/flop scale.

---

## Phase 4: Turn Solver

### Context

One card to come (~44-46 possible river cards). For each river card, hand strengths change. Builds on Phase 3 — the river sub-trees are solved as part of the turn tree.

### How It Works

The turn game tree has **chance nodes** where the river card is dealt:

```
Turn Action Tree (same structure as river)
└─ After all turn actions resolve to "see river" →
   Chance Node: deal river card (44-46 possibilities)
   └─ For each river card → River Action Tree → Showdown
```

CFR+ traverses the **entire turn+river tree** in each iteration. It does NOT solve each river independently — the strategies are interdependent.

### Scale

- ~1,000 hand combos × ~45 river cards × ~40 tree nodes per street × 2 streets
- **Info sets: ~500K-2M**
- **Memory: 50-200 MB**

### Key Optimization: Flat Array CFR Storage

The current `HashMap<InfoSetKey, InfoSetData>` with `Vec<f64>` per info set won't scale. Phase 4 requires:

```rust
// BEFORE (current): ~120 bytes per info set + HashMap overhead
pub struct CfrTrainer {
    info_sets: HashMap<InfoSetKey, InfoSetData>,  // heap alloc per entry
}

// AFTER: ~24 bytes per info set, zero overhead
pub struct FlatCfr {
    regrets: Vec<f32>,    // all regrets in one contiguous array
    strategy: Vec<f32>,   // all strategies in one contiguous array
    num_actions: Vec<u8>, // actions per info set
    offsets: Vec<u32>,    // index into regrets/strategy arrays
}
```

This gives ~5x memory reduction and much better cache performance.

### Decisions

- **Full river enumeration** (not sampling): Only ~45 river cards, feasible to enumerate all of them exactly. More accurate than Monte Carlo sampling.
- **f32 storage**: Switch from f64 to f32 for regrets and strategies. Halves memory, no meaningful accuracy loss.
- **Flat array CFR**: Replace HashMap with contiguous arrays (see above).
- **ahash**: Switch to fast hasher for any remaining HashMap usage.

### Performance

- **Solve time**: 15-45 seconds per spot at 50K iterations
- **Accuracy**: Exploitability < 0.5% pot
- **Memory**: 50-200 MB per solve
- **Query**: Instant from cache

### Files

| File | Action |
|---|---|
| `src/flat_cfr.rs` | NEW — flat array CFR engine (replaces HashMap-based for postflop) |
| `src/postflop_solver.rs` | MODIFY — add turn solver, chance node handling |
| `src/postflop_tree.rs` | MODIFY — add chance nodes for river card |
| `src/cli.rs` | MODIFY — turn support in solve/strategy commands |
| `tests/test_turn_solver.rs` | NEW |

### Steps

**4.1**: Flat array CFR engine (`src/flat_cfr.rs`)
- `FlatCfr` struct with f32 contiguous arrays
- Same CFR+ algorithm, but ~5x less memory and better cache perf
- Regret matching, average strategy, update — all using flat indexing

**4.2**: Chance node support in game tree (`src/postflop_tree.rs`)
- Add `Chance` variant to `PostflopNode`
- Enumerate all possible river cards (excluding board + hand blockers)
- Weight each river outcome equally (uniform chance)

**4.3**: Turn solver (`src/postflop_solver.rs`)
- `TurnSolver`: builds turn+river tree, runs CFR+ through entire tree
- Each iteration traverses turn actions → chance node → all river subtrees
- Hand strength re-evaluated for each river card using `evaluate_fast()`

**4.4**: Turn caching + CLI
- Cache to `~/.gto-cli/solver/turn_{board}_{pot}_{stack}.json`
- `gto solve postflop --board XsXdXcXd` detects 4-card board → turn solve

**4.5**: Tests
- Convergence: exploitability < 0.5% pot
- Drawing hands bet more on turn than river (semi-bluff)
- Made hands with draws bet bigger (protection)
- Strategy changes based on river card (e.g., flush-completing river changes action)

---

## Phase 5: Flop Solver

### Context

Two streets of chance nodes (turn + river). The full game tree: flop actions → turn card → turn actions → river card → river actions → showdown. This is what PioSolver primarily solves.

### Scale

- ~1,000 hand combos × ~47 turn cards × ~46 river cards = ~2,162 board runouts
- Game tree across 3 streets with 2-3 bet sizes each
- **Info sets: 5M-50M** (depending on bet tree complexity)
- **Memory: 0.5-5 GB** (target: 0.5-1.5 GB with optimizations)

### Key Optimizations Required

| Optimization | What | Impact |
|---|---|---|
| Flat array CFR (Phase 4) | Contiguous f32 storage | 5x memory reduction |
| Hand bucketing | Cluster similar hands by equity | 5-10x info set reduction |
| Regret pruning | Skip traversal of low-regret subtrees | 2-3x speed per iteration |
| Rayon parallelization | Parallelize CFR iterations across hands | Linear speedup with cores |
| External sampling MCCFR | Sample chance nodes instead of full enumeration | 10-50x cheaper per iteration |

### Hand Bucketing (Card Abstraction)

Instead of tracking strategies for all ~1,000 individual hand combos, cluster similar hands into ~150-200 **buckets** based on equity characteristics.

**Method**: Opponent Cluster Hand Strength (OCHS)
1. For each hand on this board, compute equity vs a uniform random opponent
2. Cluster hands into N buckets by equity similarity (k-means)
3. All hands in the same bucket share the same strategy

**Bucket counts**:
- Flop: ~200 buckets (from ~1000 combos)
- Turn: ~200 buckets
- River: ~200 buckets (or exact combos since river is small)

This reduces info sets from 50M to ~5M, fitting in 0.5-1.5 GB.

### Algorithm: External Sampling MCCFR

For flop, full tree traversal each iteration is too expensive (~2,162 runouts × full tree). Switch to **external sampling MCCFR**:

- Each iteration: sample ONE turn card and ONE river card
- Traverse only that single runout's tree
- Much cheaper per iteration (~2000x), but needs more iterations to converge
- Standard approach used by all commercial solvers for flop-level trees

**Iteration count**: 500K-2M iterations (each one is cheap due to sampling)

### Decisions

- **Bucketing method**: OCHS (equity-based clustering). ~200 buckets per street.
- **MCCFR vs CFR+**: Use external sampling MCCFR for flop. CFR+ for river/turn (small enough for full traversal).
- **Bet sizes**: Flop: 33%, 75% pot. Turn: 50%, 100% pot. River: 33%, 66%, 100% pot.
- **Target memory**: 0.5-1.5 GB per flop solve (fits 16 GB laptop comfortably).

### Performance

- **Solve time**: 1-4 minutes per flop spot on Railway (8 vCPU)
- **Accuracy**: Exploitability < 1% pot after 1M MCCFR iterations
- **Memory**: 0.5-1.5 GB per solve
- **Query**: Instant from cache

### Files

| File | Action |
|---|---|
| `src/bucketing.rs` | NEW — hand clustering (OCHS, k-means) |
| `src/mccfr.rs` | NEW — external sampling MCCFR algorithm |
| `src/postflop_solver.rs` | MODIFY — add flop solver using MCCFR + bucketing |
| `src/postflop_tree.rs` | MODIFY — two levels of chance nodes |
| `src/cli.rs` | MODIFY — flop support |
| `tests/test_flop_solver.rs` | NEW |

### Steps

**5.1**: Hand bucketing (`src/bucketing.rs`)
- Compute equity for all hand combos vs uniform range on a given board
- K-means clustering into N buckets
- `HandBucketing` struct: maps each combo to a bucket index
- Test: similar equity hands land in same bucket, dissimilar hands in different buckets

**5.2**: External sampling MCCFR (`src/mccfr.rs`)
- `MccfrTrainer`: similar interface to `FlatCfr` but with sampling
- Each iteration: sample chance outcomes, traverse one path through tree
- Regret update only on the sampled path
- Average strategy accumulated across all samples

**5.3**: Flop solver (`src/postflop_solver.rs`)
- `FlopSolver`: builds full 3-street tree with bucketing
- Integrates MCCFR for iteration, hand buckets for abstraction
- Flop → turn chance → turn → river chance → river → showdown

**5.4**: Flop caching + CLI
- Cache to `~/.gto-cli/solver/flop_{board}_{scenario}_{pot}_{stack}.json`
- `gto solve postflop --board XsXdXc` detects 3-card board → flop solve
- Compressed storage (flop solutions are larger)

**5.5**: Tests
- Convergence: exploitability < 1% pot
- C-bet frequency on dry vs wet boards
- Continuation across streets (flop bet → turn bet sequences)
- Check-raise frequency reasonable (5-15%)
- Position advantage: IP profits more than OOP on average

---

## Phase 6: CFR Engine Optimization

Performance-critical optimizations applied after core functionality works.

### Steps

**6.1**: SIMD batch evaluation
- Use SIMD intrinsics for batch hand evaluation (evaluate 4-8 hands simultaneously)
- Target: 100M+ evals/sec (up from 10-50M)

**6.2**: Parallel CFR iterations
- Rayon parallelization of hand traversal within each CFR iteration
- Each hand's subtree traversed independently on separate threads
- Expected: ~4-6x speedup on 8-core Railway instances

**6.3**: Regret pruning
- Skip traversal of actions with cumulative regret < threshold
- Re-check periodically (every 1000 iterations)
- Expected: 2-3x speedup per iteration

**6.4**: Compressed cache format
- Switch from JSON to bincode or MessagePack for cached solutions
- Expected: 5-10x smaller files, faster load times
- JSON kept as optional export format

---

## Phase 7: Railway Deployment + Batch Pre-Solve

### Architecture

```
Railway Instance (Pro plan, 32 GB RAM, 8 vCPU)
├── Solver binary (compiled for linux-x86_64)
├── Job queue: list of spots to solve
├── Progress tracking
├── Persistent volume: cached solutions
└── API endpoint OR rsync access for downloading results
```

### Steps

**7.1**: Dockerfile + Railway config
- Multi-stage Rust build (compile in builder, copy binary to slim runtime)
- Railway persistent volume mounted at `/data/solver/`
- Environment vars for config (iterations, bet sizes, etc.)

**7.2**: Batch solve orchestrator
- `gto solve batch` command: takes a manifest of spots to solve
- Solves preflop first (~3 min), then flops in priority order
- Progress output: `[47/3000] Solving AsKd7c BTN vs BB ... done (1.2 min)`
- Spots usable immediately as each one finishes
- Resumable: skips already-cached spots on restart

**7.3**: Flop prioritization
- Solve most common flop textures first:
  1. High-card boards (A-high, K-high): ~40 flops
  2. Paired boards: ~30 flops
  3. Connected boards: ~40 flops
  4. Monotone/two-tone: ~40 flops
  5. Low boards: ~50 flops
  6. Everything else: remaining ~1,555

**7.4**: Download + local storage
- `gto sync` command: rsync cached solutions from Railway to laptop
- Or: Railway exposes simple HTTP API, CLI fetches solutions on demand
- Local cache at `~/.gto-cli/solver/`
- Incremental: only downloads new/updated solutions

### Cost Estimate

| Tier | Spots | Railway Time | Cost |
|---|---|---|---|
| Tier 1 (60% coverage) | 250 | ~6 hours | ~$2-7 |
| Tier 2 (90% coverage) | 3,000 | ~2 days | ~$20-40 |
| Full (100%) | 52,650 | ~2 weeks | ~$300-750 |

After pre-solve: kill the instance, stop paying. Solutions stored locally forever.

---

## Phase 8: Unified CLI Query Interface

### Overview

Two modes of interaction, both reading from the same cache. **All solutions are cached after first solve** — the first time a spot is solved (either via pre-solve on Railway or on-the-fly), the result is saved to disk. Every subsequent query for that spot is instant. The cache grows over time as you play more hands.

### Mode 1: Interactive Play Mode (Primary — for live play)

`gto play` — a stateful interactive session that tracks your hand through every street. You only input what changes. The CLI handles pot math, stack tracking, and street detection automatically.

```bash
$ gto play
Stack? 100

Hand? AhQd
Position? BTN
→ RAISE 2.5bb (100%)

# Tell the CLI what happened
Action? raise 2.5
Villain? BB calls
# CLI auto-computes: pot=6, hero_stack=97.5

Board? Ks9d4c
→ BET 33% (72%), CHECK (28%)

Action? bet 33
Villain? call
# CLI auto-computes: pot=10, hero_stack=95.5

Board? 7h
→ CHECK (55%), BET 66% (45%)

Action? check
Villain? check
# Pot stays 10.

Board? Qc
→ BET 66% (80%), BET 33% (12%), CHECK (8%)

Action? bet 66
Villain? raise 20
→ CALL (85%), FOLD (15%)

Action? call
# Hand over. CLI shows result.

# Next hand — stack carries over from previous hand
Hand? JcTs
Position? CO
→ RAISE 2.5bb (92%), FOLD (8%)
```

**State tracked automatically:**
- Hero stack (carries across hands, updated by wins/losses)
- Pot size (updated by each action)
- Board cards (appended each street)
- Villain range (narrowed by their actions — e.g., BB cold-called preflop → use BB defend range)
- Street (detected from board card count)
- Position / IP-OOP (set once per hand)

**Commands within interactive mode:**
- `Hand? XxXx` — start new hand
- `Board? XxXx` — add board cards (new street)
- `Action? bet 33 / check / call / fold / raise 20` — what hero did
- `Villain? call / raise 15 / bet 10 / check / fold` — what villain did
- `stack 95` — manually override stack
- `pot 20` — manually override pot
- `new` — new hand (reset board, keep stack)
- `quit` — exit

### Mode 2: One-Shot Commands (for quick lookups / study)

**Positional shorthand** (fastest to type):
```bash
gto AhQd BTN                           # preflop
gto AhQd BTN Ks9d4c 6                  # flop, pot=6
gto AhQd BTN Ks9d4c7h 10              # turn, pot=10
gto AhQd BTN Ks9d4c7hQc 10            # river, pot=10
```

Format: `gto <HAND> <POSITION> [BOARD] [POT] [STACK]`
- Stack defaults to 100bb if omitted
- Villain defaults based on position context

**Flag syntax** (for precision):
```bash
gto action AhQd --position BTN --board Ks9d4c7hQc --pot 10 --stack 95.5 --vs BB
```

### Caching Behavior

**Every query returns in 1-3 seconds, always.** No exceptions, no blocking solves. The cache is permanent and grows over time.

```
Query for a spot:
  ├─ Cache hit?  → Return instantly (<100ms)
  └─ Cache miss? → Return closest cached match instantly (<1 sec)
                    + Queue background solve for exact spot
                    + Next query for this spot → exact cache hit
```

**Cache location**: `~/.gto-cli/solver/`

**After pre-solving on Railway** (Tier 2: ~3,000 spots), ~90% of queries are exact cache hits. The remaining ~10% use closest-match approximation the first time (95%+ same answer), then become exact cache hits after the background solve completes. The more you play, the fewer approximations — after a few weeks of regular use, virtually everything is cached.

**Cache is portable**: Copy `~/.gto-cli/solver/` to any machine. Share with friends. Back up to cloud storage. It's just JSON files on disk.

### Closest-Match Fallback (All Streets)

When ANY spot (river, turn, or flop) isn't cached, the system **always returns instantly** using the closest cached match. It never blocks on a solve.

Matching criteria:
1. **Board texture similarity**:
   - Same high card rank
   - Same suit pattern (monotone/two-tone/rainbow)
   - Same connectedness (connected/gapped/disconnected)
2. **Same street** (river matches river, turn matches turn, etc.)
3. **Closest SPR** (stack-to-pot ratio)
4. **Same position matchup** (BTN vs BB, etc.)

Display with a disclaimer:
```
(closest match: Ks9d4c — yours: Ks9d5c)
→ BET 33% (72%), CHECK (28%)
Solving exact spot in background...
```

Background solve priority:
- River: completes in 1-5 sec (cached almost immediately)
- Turn: completes in 15-45 sec (cached before next hand)
- Flop: completes in 1-4 min (cached for next session)

### Steps

**8.1**: Interactive play mode (`gto play` rewrite)
- Replace existing `gto play` with solver-backed interactive mode
- State machine: tracks hand, position, board, pot, stack, villain range
- Parse action inputs: "bet 33", "check", "call", "raise 20", "fold"
- Parse villain inputs: "call", "raise 15", "bet 10", "check", "fold"
- Auto-compute pot and stack after each action
- Detect street from board card count, route to correct solver cache

**8.2**: One-shot positional command
- `gto <HAND> <POSITION> [BOARD] [POT] [STACK]` — positional args, no flags
- Smart defaults: stack=100bb, villain inferred from position
- Detect street from board length (0=preflop, 6chars=flop, 8=turn, 10=river)

**8.3**: Cache-or-solve logic (always-instant)
- Check cache first (instant, <100ms)
- On miss for ANY street: return closest cached match instantly (<1 sec)
- Queue background solve for exact spot (async, non-blocking)
- Background solve caches result — next query for this spot is exact
- Closest-match scoring: board texture similarity + SPR proximity + same position matchup
- River background solves complete in ~1-5 sec, turn ~15-45 sec, flop ~1-4 min

**8.4**: Villain range narrowing
- Track villain actions through the hand
- Preflop: "BB calls" → load BB defend range from preflop solver
- Postflop: use the cached strategy to infer villain's continuing range
- Each street narrows the range based on what villain did

---

## Performance Summary

### Solve Times (one-time, background on Railway)

| Street | Per Spot | Iterations | Memory | Accuracy |
|---|---|---|---|---|
| Preflop | 3-5 sec | 50K | <1 MB | <0.15 bb exploit |
| River | 1-5 sec | 10K | <10 MB | <0.1% pot |
| Turn | 15-45 sec | 50K | 50-200 MB | <0.5% pot |
| Flop | 1-4 min | 1M MCCFR | 0.5-1.5 GB | <1% pot |

### Query Times (user experience, from cache)

**Every query: <1 second.** Always. Regardless of street.

### Total Pre-Solve for 90% Coverage

- **Time**: ~2 days on Railway
- **Cost**: ~$20-40
- **Storage**: 20-80 GB on disk
- **Result**: Instant GTO answers for ~90% of poker situations, forever, offline

---

## Key Decisions Summary

| Decision | Choice | Rationale |
|---|---|---|
| CFR storage (postflop) | Flat f32 arrays | 10x less memory than HashMap + f64 |
| River algorithm | CFR+ (full traversal) | Small tree, fast convergence |
| Turn algorithm | CFR+ (full river enumeration) | Only ~45 river cards, exact is feasible |
| Flop algorithm | External sampling MCCFR | Too many runouts for full traversal |
| Hand abstraction | OCHS bucketing, ~200 per street | Needed for flop memory budget |
| Bet sizes | 33/66/100% pot (configurable) | Balance accuracy vs tree size |
| Pre-solve target | Tier 2: 200 flops × 15 scenarios | 90% coverage, practical cost |
| Deployment | Railway Pro (32 GB, 8 vCPU) | Cheap, sufficient for Tier 2 |
| Cache format | JSON initially, bincode later | JSON for debugging, bincode for production |
| Range input | From preflop solver output | Automatic, no manual range entry |
| Caching | Cache every solve permanently | First solve caches; every repeat query is instant forever |
| Primary UX | Interactive `gto play` mode | Tracks pot/stack/board automatically, minimal typing |
| Secondary UX | Positional one-shot `gto AhQd BTN ...` | For quick lookups and study |
| Cache miss (all streets) | Always return closest match instantly + background solve | Every query 1-3 sec, no exceptions. Background solve caches exact answer for next time |
| Stack tracking | Auto-carry across hands | No manual pot/stack entry after first hand |
