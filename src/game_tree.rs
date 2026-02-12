//! Push/fold game tree and solver.
//!
//! Implements a CFR+ solver for the simplest poker decision:
//! SB shoves all-in or folds, BB calls or folds. Produces Nash
//! equilibrium push/call ranges for any stack depth.

use rand::Rng;
use rand::SeedableRng;
use rand::rngs::StdRng;
use rayon::prelude::*;

use crate::card_encoding::{card_to_index, remaining_deck};
use crate::cards::hand_combos;
use crate::cfr::{CfrTrainer, InfoSetKey};
use crate::lookup_eval::evaluate_fast;
use crate::ranges::combo_count;

/// The 13 ranks in grid order: A, K, Q, J, T, 9, 8, 7, 6, 5, 4, 3, 2.
const GRID_RANKS: [char; 13] = [
    'A', 'K', 'Q', 'J', 'T', '9', '8', '7', '6', '5', '4', '3', '2',
];

/// Number of canonical preflop hands (13x13 grid).
pub const NUM_HANDS: usize = 169;

/// Node IDs in the push/fold game tree.
const SB_NODE: u16 = 0;
const BB_NODE: u16 = 1;

// ---------------------------------------------------------------------------
// Hand bucket mapping
// ---------------------------------------------------------------------------

/// Map a canonical hand notation (e.g., "AA", "AKs", "72o") to its
/// bucket index (0-168) in the 13x13 grid.
///
/// Grid layout:
///   - Diagonal (row == col): pairs (AA, KK, ...)
///   - Above diagonal (row < col): suited hands (AKs, AQs, ...)
///   - Below diagonal (row > col): offsuit hands (AKo, AQo, ...)
pub fn hand_to_bucket(notation: &str) -> Option<usize> {
    let chars: Vec<char> = notation.chars().collect();
    if chars.len() < 2 {
        return None;
    }

    let r1_idx = GRID_RANKS.iter().position(|&r| r == chars[0])?;
    let r2_idx = GRID_RANKS.iter().position(|&r| r == chars[1])?;

    if chars.len() == 2 && chars[0] == chars[1] {
        return Some(r1_idx * 13 + r1_idx);
    }

    if chars.len() == 3 {
        match chars[2] {
            's' => {
                let (row, col) = (r1_idx.min(r2_idx), r1_idx.max(r2_idx));
                Some(row * 13 + col)
            }
            'o' => {
                let (row, col) = (r1_idx.max(r2_idx), r1_idx.min(r2_idx));
                Some(row * 13 + col)
            }
            _ => None,
        }
    } else {
        None
    }
}

/// Get canonical hand notation from bucket index (0-168).
pub fn bucket_to_hand(bucket: usize) -> String {
    let row = bucket / 13;
    let col = bucket % 13;
    if row == col {
        format!("{}{}", GRID_RANKS[row], GRID_RANKS[col])
    } else if row < col {
        format!("{}{}s", GRID_RANKS[row], GRID_RANKS[col])
    } else {
        format!("{}{}o", GRID_RANKS[col], GRID_RANKS[row])
    }
}

// ---------------------------------------------------------------------------
// Equity and combo weight precomputation
// ---------------------------------------------------------------------------

/// Precomputed equity and combo weight tables for 169x169 canonical matchups.
pub struct EquityTable {
    /// equity[i * 169 + j] = average equity of hand i vs hand j.
    pub equity: Vec<f64>,
    /// combos[i * 169 + j] = number of non-conflicting combo pairs.
    pub combos: Vec<f64>,
}

impl EquityTable {
    #[inline]
    pub fn eq(&self, i: usize, j: usize) -> f64 {
        self.equity[i * NUM_HANDS + j]
    }

    #[inline]
    pub fn weight(&self, i: usize, j: usize) -> f64 {
        self.combos[i * NUM_HANDS + j]
    }
}

/// Precompute the 169x169 equity and combo weight tables using Monte Carlo.
///
/// For each pair of canonical hands, enumerates non-conflicting combo pairs
/// and runs `mc_samples` random board runouts to estimate showdown equity.
pub fn precompute_equity_table(mc_samples: usize) -> EquityTable {
    // Generate all combos for each canonical hand as u8 card indices.
    let hand_combos_list: Vec<Vec<[u8; 2]>> = (0..NUM_HANDS)
        .map(|bucket| {
            let notation = bucket_to_hand(bucket);
            hand_combos(&notation)
                .unwrap_or_default()
                .iter()
                .map(|(c1, c2)| [card_to_index(c1), card_to_index(c2)])
                .collect()
        })
        .collect();

    // Compute each row in parallel using rayon.
    let rows: Vec<(Vec<f64>, Vec<f64>)> = (0..NUM_HANDS)
        .into_par_iter()
        .map(|i| {
            let mut eq_row = vec![0.5f64; NUM_HANDS];
            let mut combo_row = vec![0.0f64; NUM_HANDS];
            let mut rng = StdRng::seed_from_u64(i as u64);

            let combos_i = &hand_combos_list[i];

            for j in 0..NUM_HANDS {
                let combos_j = &hand_combos_list[j];

                // Find all non-conflicting combo pairs.
                let mut valid_pairs: Vec<[u8; 4]> = Vec::new();
                for ci in combos_i {
                    for cj in combos_j {
                        if ci[0] != cj[0]
                            && ci[0] != cj[1]
                            && ci[1] != cj[0]
                            && ci[1] != cj[1]
                        {
                            valid_pairs.push([ci[0], ci[1], cj[0], cj[1]]);
                        }
                    }
                }

                combo_row[j] = valid_pairs.len() as f64;

                if valid_pairs.is_empty() {
                    eq_row[j] = 0.5;
                    continue;
                }

                // Monte Carlo equity estimation.
                let mut wins = 0u32;
                let mut ties = 0u32;
                let total = mc_samples as u32;

                for _ in 0..mc_samples {
                    let pair = valid_pairs[rng.gen_range(0..valid_pairs.len())];

                    let mut deck = remaining_deck(&pair);
                    // Shuffle first 5 elements (partial Fisher-Yates).
                    for k in 0..5 {
                        let swap = rng.gen_range(k..deck.len());
                        deck.swap(k, swap);
                    }

                    let h1 = [
                        pair[0], pair[1], deck[0], deck[1], deck[2], deck[3], deck[4],
                    ];
                    let h2 = [
                        pair[2], pair[3], deck[0], deck[1], deck[2], deck[3], deck[4],
                    ];

                    let s1 = evaluate_fast(&h1);
                    let s2 = evaluate_fast(&h2);

                    if s1 > s2 {
                        wins += 1;
                    } else if s1 == s2 {
                        ties += 1;
                    }
                }

                eq_row[j] = (wins as f64 + 0.5 * ties as f64) / total as f64;
            }

            (eq_row, combo_row)
        })
        .collect();

    // Flatten into contiguous arrays.
    let mut equity = vec![0.0f64; NUM_HANDS * NUM_HANDS];
    let mut combos = vec![0.0f64; NUM_HANDS * NUM_HANDS];

    for (i, (eq_row, combo_row)) in rows.into_iter().enumerate() {
        equity[i * NUM_HANDS..(i + 1) * NUM_HANDS].copy_from_slice(&eq_row);
        combos[i * NUM_HANDS..(i + 1) * NUM_HANDS].copy_from_slice(&combo_row);
    }

    EquityTable { equity, combos }
}

// ---------------------------------------------------------------------------
// Push/fold payoffs
// ---------------------------------------------------------------------------

/// Terminal payoffs for the push/fold game tree (in bb, from SB's perspective).
///
/// Blinds: SB posts 0.5bb, BB posts 1bb. Both start with `stack` bb.
///
/// - SB folds: SB = -0.5, BB = +0.5
/// - SB pushes, BB folds: SB = +1.0, BB = -1.0
/// - SB pushes, BB calls: showdown for 2*stack pot (minus rake)
pub struct PushFoldPayoffs {
    pub stack_bb: f64,
    pub rake: f64, // as fraction (0.0 - 1.0)
}

impl PushFoldPayoffs {
    pub fn new(stack_bb: f64, rake_pct: f64) -> Self {
        PushFoldPayoffs {
            stack_bb,
            rake: rake_pct / 100.0,
        }
    }

    /// SB folds: loses small blind.
    #[inline]
    pub fn sb_fold(&self) -> f64 {
        -0.5
    }

    /// SB pushes, BB folds: SB wins BB's blind.
    #[inline]
    pub fn sb_push_bb_fold(&self) -> f64 {
        1.0
    }

    /// BB folds vs push: loses big blind.
    #[inline]
    pub fn bb_fold(&self) -> f64 {
        -1.0
    }

    /// SB's payoff at showdown given SB's equity.
    /// payoff = stack * (2 * equity * (1 - rake) - 1)
    #[inline]
    pub fn sb_showdown(&self, sb_equity: f64) -> f64 {
        self.stack_bb * (2.0 * sb_equity * (1.0 - self.rake) - 1.0)
    }

    /// BB's payoff at showdown given SB's equity.
    /// payoff = stack * (2 * (1 - sb_equity) * (1 - rake) - 1)
    #[inline]
    pub fn bb_showdown(&self, sb_equity: f64) -> f64 {
        self.stack_bb * (2.0 * (1.0 - sb_equity) * (1.0 - self.rake) - 1.0)
    }
}

// ---------------------------------------------------------------------------
// Push/fold solver
// ---------------------------------------------------------------------------

/// Result of solving a push/fold game.
pub struct PushFoldResult {
    /// Push probability for each SB hand bucket (0-168).
    /// Index 0 = push probability for the hand at bucket 0.
    pub push_strategy: Vec<f64>,
    /// Call probability for each BB hand bucket (0-168).
    pub call_strategy: Vec<f64>,
    /// Exploitability in bb per hand (0 = Nash equilibrium).
    pub exploitability: f64,
    /// Number of CFR iterations run.
    pub iterations: usize,
    /// Effective stack in bb.
    pub stack_bb: f64,
}

impl PushFoldResult {
    /// Hands that SB should push (>50% push frequency).
    pub fn push_range(&self) -> Vec<String> {
        (0..NUM_HANDS)
            .filter(|&i| self.push_strategy[i] > 0.5)
            .map(bucket_to_hand)
            .collect()
    }

    /// Hands that BB should call (>50% call frequency).
    pub fn call_range(&self) -> Vec<String> {
        (0..NUM_HANDS)
            .filter(|&i| self.call_strategy[i] > 0.5)
            .map(bucket_to_hand)
            .collect()
    }

    /// Total push range as percentage of all hands.
    pub fn push_pct(&self) -> f64 {
        let combos: f64 = (0..NUM_HANDS)
            .filter(|&i| self.push_strategy[i] > 0.5)
            .map(|i| combo_count(&bucket_to_hand(i)) as f64)
            .sum();
        combos / 1326.0 * 100.0
    }

    /// Total call range as percentage of all hands.
    pub fn call_pct(&self) -> f64 {
        let combos: f64 = (0..NUM_HANDS)
            .filter(|&i| self.call_strategy[i] > 0.5)
            .map(|i| combo_count(&bucket_to_hand(i)) as f64)
            .sum();
        combos / 1326.0 * 100.0
    }

    /// Display the solver results: push/call grids and summary stats.
    pub fn display(&self) {
        use colored::Colorize;
        use crate::display::{range_grid, strategy_grid};

        println!();
        println!(
            "  {} Push/Fold Solution  |  Stack: {}bb  |  {} iterations  |  Exploitability: {:.4} bb",
            "GTO".bold(),
            self.stack_bb,
            self.iterations,
            self.exploitability,
        );

        // SB push range
        let push_range = self.push_range();
        println!();
        println!("{}", range_grid(&push_range, &format!(
            "SB Push Range ({:.1}% of hands)", self.push_pct()
        )));

        // SB push frequency grid
        println!();
        println!("{}", strategy_grid(
            &self.push_strategy,
            "SB Push Frequency (%)",
        ));

        // BB call range
        let call_range = self.call_range();
        println!();
        println!("{}", range_grid(&call_range, &format!(
            "BB Call Range ({:.1}% of hands)", self.call_pct()
        )));

        // BB call frequency grid
        println!();
        println!("{}", strategy_grid(
            &self.call_strategy,
            "BB Call Frequency (%)",
        ));

        println!();
    }
}

/// Solve the push/fold game for a given stack depth using CFR+.
///
/// Returns Nash equilibrium push/call ranges.
pub fn solve_push_fold(stack_bb: f64, iterations: usize, rake_pct: f64) -> PushFoldResult {
    let payoffs = PushFoldPayoffs::new(stack_bb, rake_pct);

    // Step 1: Precompute equity table (the expensive part).
    let table = precompute_equity_table(2000);

    // Step 2: Run CFR+ iterations.
    let mut trainer = CfrTrainer::new();

    // Pre-create all info sets.
    for h in 0..NUM_HANDS {
        trainer.get_or_create(
            &InfoSetKey { hand_bucket: h as u16, node_id: SB_NODE },
            2,
        );
        trainer.get_or_create(
            &InfoSetKey { hand_bucket: h as u16, node_id: BB_NODE },
            2,
        );
    }

    for _ in 0..iterations {
        cfr_iteration(&mut trainer, &table, &payoffs);
    }

    // Step 3: Extract average strategies.
    let push_strategy: Vec<f64> = (0..NUM_HANDS)
        .map(|h| {
            let key = InfoSetKey { hand_bucket: h as u16, node_id: SB_NODE };
            trainer.get_average_strategy(&key, 2)[0]
        })
        .collect();

    let call_strategy: Vec<f64> = (0..NUM_HANDS)
        .map(|h| {
            let key = InfoSetKey { hand_bucket: h as u16, node_id: BB_NODE };
            trainer.get_average_strategy(&key, 2)[0]
        })
        .collect();

    // Step 4: Compute exploitability.
    let exploitability = compute_exploitability(
        &push_strategy,
        &call_strategy,
        &table,
        &payoffs,
    );

    PushFoldResult {
        push_strategy,
        call_strategy,
        exploitability,
        iterations,
        stack_bb,
    }
}

/// Run one CFR+ iteration: update all SB and BB info sets.
fn cfr_iteration(trainer: &mut CfrTrainer, table: &EquityTable, payoffs: &PushFoldPayoffs) {
    // Snapshot current strategies to avoid borrow conflicts.
    let bb_strats: Vec<[f64; 2]> = (0..NUM_HANDS)
        .map(|h| {
            let s = trainer.get_strategy(
                &InfoSetKey { hand_bucket: h as u16, node_id: BB_NODE },
                2,
            );
            [s[0], s[1]]
        })
        .collect();

    // --- Update SB info sets ---
    for sb in 0..NUM_HANDS {
        let sb_key = InfoSetKey { hand_bucket: sb as u16, node_id: SB_NODE };
        let sb_strat = trainer.get_strategy(&sb_key, 2);

        let mut push_value = 0.0;
        let fold_value = payoffs.sb_fold();
        let mut total_w = 0.0;

        for bb in 0..NUM_HANDS {
            let w = table.weight(sb, bb);
            if w < 1e-10 {
                continue;
            }
            total_w += w;

            let eq = table.eq(sb, bb);
            let bb_call_prob = bb_strats[bb][0];
            let bb_fold_prob = bb_strats[bb][1];

            let ev_push = bb_fold_prob * payoffs.sb_push_bb_fold()
                + bb_call_prob * payoffs.sb_showdown(eq);

            push_value += w * ev_push;
        }

        if total_w > 0.0 {
            push_value /= total_w;
        }

        let node_value = sb_strat[0] * push_value + sb_strat[1] * fold_value;

        let data = trainer.get_or_create(&sb_key, 2);
        data.update(&[push_value, fold_value], node_value, 1.0);
    }

    // Snapshot SB strategies for BB update.
    let sb_strats: Vec<[f64; 2]> = (0..NUM_HANDS)
        .map(|h| {
            let s = trainer.get_strategy(
                &InfoSetKey { hand_bucket: h as u16, node_id: SB_NODE },
                2,
            );
            [s[0], s[1]]
        })
        .collect();

    // --- Update BB info sets ---
    for bb in 0..NUM_HANDS {
        let bb_key = InfoSetKey { hand_bucket: bb as u16, node_id: BB_NODE };
        let bb_strat = trainer.get_strategy(&bb_key, 2);

        let mut call_value = 0.0;
        let fold_value = payoffs.bb_fold();
        let mut total_w = 0.0;

        for sb in 0..NUM_HANDS {
            let w = table.weight(sb, bb);
            if w < 1e-10 {
                continue;
            }

            let push_prob = sb_strats[sb][0];
            if push_prob < 1e-10 {
                continue;
            }

            total_w += w * push_prob;

            let eq = table.eq(sb, bb);
            call_value += w * push_prob * payoffs.bb_showdown(eq);
        }

        if total_w > 0.0 {
            call_value /= total_w;
        }

        let node_value = bb_strat[0] * call_value + bb_strat[1] * fold_value;

        let data = trainer.get_or_create(&bb_key, 2);
        data.update(&[call_value, fold_value], node_value, 1.0);
    }
}

/// Compute exploitability: how much each player could gain by deviating
/// to a best-response strategy. Returns value in bb per hand.
fn compute_exploitability(
    push_strat: &[f64],
    call_strat: &[f64],
    table: &EquityTable,
    payoffs: &PushFoldPayoffs,
) -> f64 {
    let mut sb_gain = 0.0;
    let mut sb_total_combos = 0.0;

    // SB best response against BB's fixed call strategy.
    for sb in 0..NUM_HANDS {
        let combos = combo_count(&bucket_to_hand(sb)) as f64;

        let mut push_ev = 0.0;
        let fold_ev = payoffs.sb_fold();
        let mut total_w = 0.0;

        for bb in 0..NUM_HANDS {
            let w = table.weight(sb, bb);
            if w < 1e-10 {
                continue;
            }
            total_w += w;

            let eq = table.eq(sb, bb);
            let bb_call = call_strat[bb];
            let bb_fold = 1.0 - bb_call;

            push_ev += w * (bb_fold * payoffs.sb_push_bb_fold()
                + bb_call * payoffs.sb_showdown(eq));
        }

        if total_w > 0.0 {
            push_ev /= total_w;
        }

        let current_ev = push_strat[sb] * push_ev + (1.0 - push_strat[sb]) * fold_ev;
        let best_ev = push_ev.max(fold_ev);

        sb_gain += combos * (best_ev - current_ev);
        sb_total_combos += combos;
    }

    // BB best response against SB's fixed push strategy.
    let mut bb_gain = 0.0;
    let mut bb_total_combos = 0.0;

    for bb in 0..NUM_HANDS {
        let combos = combo_count(&bucket_to_hand(bb)) as f64;

        let mut call_ev = 0.0;
        let fold_ev = payoffs.bb_fold();
        let mut total_w = 0.0;

        for sb in 0..NUM_HANDS {
            let w = table.weight(sb, bb);
            if w < 1e-10 {
                continue;
            }

            let push_prob = push_strat[sb];
            if push_prob < 1e-10 {
                continue;
            }

            total_w += w * push_prob;

            let eq = table.eq(sb, bb);
            call_ev += w * push_prob * payoffs.bb_showdown(eq);
        }

        if total_w > 0.0 {
            call_ev /= total_w;
        }

        let current_ev = call_strat[bb] * call_ev + (1.0 - call_strat[bb]) * fold_ev;
        let best_ev = call_ev.max(fold_ev);

        bb_gain += combos * (best_ev - current_ev);
        bb_total_combos += combos;
    }

    (sb_gain / sb_total_combos + bb_gain / bb_total_combos) / 2.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bucket_roundtrip() {
        for i in 0..169 {
            let hand = bucket_to_hand(i);
            let bucket = hand_to_bucket(&hand).unwrap();
            assert_eq!(bucket, i, "roundtrip failed for bucket {} ({})", i, hand);
        }
    }

    #[test]
    fn known_buckets() {
        assert_eq!(hand_to_bucket("AA"), Some(0));
        assert_eq!(hand_to_bucket("AKs"), Some(1));
        assert_eq!(hand_to_bucket("AKo"), Some(13));
        assert_eq!(hand_to_bucket("KK"), Some(14));
        assert_eq!(hand_to_bucket("22"), Some(168));
        assert_eq!(hand_to_bucket("32o"), Some(167));
        assert_eq!(hand_to_bucket("32s"), Some(155));
    }

    #[test]
    fn bucket_to_hand_diagonal() {
        assert_eq!(bucket_to_hand(0), "AA");
        assert_eq!(bucket_to_hand(14), "KK");
        assert_eq!(bucket_to_hand(168), "22");
    }

    #[test]
    fn bucket_to_hand_suited() {
        assert_eq!(bucket_to_hand(1), "AKs");
        assert_eq!(bucket_to_hand(2), "AQs");
    }

    #[test]
    fn bucket_to_hand_offsuit() {
        assert_eq!(bucket_to_hand(13), "AKo");
        assert_eq!(bucket_to_hand(26), "AQo");
    }

    #[test]
    fn all_169_hands_unique() {
        let mut hands: Vec<String> = (0..169).map(bucket_to_hand).collect();
        hands.sort();
        hands.dedup();
        assert_eq!(hands.len(), 169);
    }

    #[test]
    fn combo_weights_non_negative() {
        let table = precompute_equity_table(100);
        for i in 0..NUM_HANDS {
            for j in 0..NUM_HANDS {
                assert!(table.weight(i, j) >= 0.0);
            }
        }
    }

    #[test]
    fn equity_in_range() {
        let table = precompute_equity_table(200);
        for i in 0..NUM_HANDS {
            for j in 0..NUM_HANDS {
                if table.weight(i, j) > 0.0 {
                    let eq = table.eq(i, j);
                    assert!(eq >= 0.0 && eq <= 1.0,
                        "equity out of range for {} vs {}: {}",
                        bucket_to_hand(i), bucket_to_hand(j), eq);
                }
            }
        }
    }

    #[test]
    fn aa_beats_random() {
        let table = precompute_equity_table(500);
        let aa = hand_to_bucket("AA").unwrap();
        // AA should have >70% equity against almost all hands.
        let mut total_eq = 0.0;
        let mut count = 0;
        for j in 0..NUM_HANDS {
            if j != aa && table.weight(aa, j) > 0.0 {
                total_eq += table.eq(aa, j);
                count += 1;
            }
        }
        let avg = total_eq / count as f64;
        assert!(avg > 0.7, "AA average equity {} should be > 0.7", avg);
    }

    #[test]
    fn payoffs_zero_sum_no_rake() {
        let p = PushFoldPayoffs::new(10.0, 0.0);
        let eq = 0.6;
        let sb = p.sb_showdown(eq);
        let bb = p.bb_showdown(eq);
        assert!((sb + bb).abs() < 1e-9, "should be zero-sum without rake");
    }

    #[test]
    fn payoffs_negative_sum_with_rake() {
        let p = PushFoldPayoffs::new(10.0, 5.0);
        let eq = 0.6;
        let sb = p.sb_showdown(eq);
        let bb = p.bb_showdown(eq);
        assert!(sb + bb < 0.0, "should be negative-sum with rake");
    }

    #[test]
    fn solver_converges() {
        // Run solver at 10bb with low iterations to verify convergence direction.
        let result = solve_push_fold(10.0, 1000, 0.0);

        // Exploitability should be small after 1000 iterations.
        assert!(
            result.exploitability < 0.5,
            "exploitability {} should be < 0.5 bb after 1000 iterations",
            result.exploitability
        );

        // AA should always push and always call.
        let aa = hand_to_bucket("AA").unwrap();
        assert!(
            result.push_strategy[aa] > 0.9,
            "AA push freq {} should be > 0.9",
            result.push_strategy[aa]
        );
        assert!(
            result.call_strategy[aa] > 0.9,
            "AA call freq {} should be > 0.9",
            result.call_strategy[aa]
        );

        // 72o should almost never push at 10bb.
        let worst = hand_to_bucket("72o").unwrap();
        assert!(
            result.push_strategy[worst] < 0.3,
            "72o push freq {} should be < 0.3 at 10bb",
            result.push_strategy[worst]
        );
    }

    #[test]
    fn solver_strategies_valid() {
        let result = solve_push_fold(10.0, 500, 0.0);

        // All strategies should be valid probabilities.
        for i in 0..NUM_HANDS {
            assert!(
                result.push_strategy[i] >= 0.0 && result.push_strategy[i] <= 1.0,
                "push_strategy[{}] = {} out of [0,1]",
                i, result.push_strategy[i]
            );
            assert!(
                result.call_strategy[i] >= 0.0 && result.call_strategy[i] <= 1.0,
                "call_strategy[{}] = {} out of [0,1]",
                i, result.call_strategy[i]
            );
        }
    }
}
