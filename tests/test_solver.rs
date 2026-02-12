//! Tests for the push/fold solver (Phase 1).
//!
//! Validates solver output against known push/fold charts and verifies
//! exploitability convergence, strategy validity, and rake effects.

use gto_cli::game_tree::{
    bucket_to_hand, hand_to_bucket, solve_push_fold, NUM_HANDS,
};
use gto_cli::ranges::combo_count;

// ---------------------------------------------------------------------------
// Helper: compute push/call percentages from a result
// ---------------------------------------------------------------------------

fn push_pct(result: &gto_cli::game_tree::PushFoldResult) -> f64 {
    let combos: f64 = (0..NUM_HANDS)
        .filter(|&i| result.push_strategy[i] > 0.5)
        .map(|i| combo_count(&bucket_to_hand(i)) as f64)
        .sum();
    combos / 1326.0 * 100.0
}

fn call_pct(result: &gto_cli::game_tree::PushFoldResult) -> f64 {
    let combos: f64 = (0..NUM_HANDS)
        .filter(|&i| result.call_strategy[i] > 0.5)
        .map(|i| combo_count(&bucket_to_hand(i)) as f64)
        .sum();
    combos / 1326.0 * 100.0
}

// ---------------------------------------------------------------------------
// Exploitability convergence
// ---------------------------------------------------------------------------

#[test]
fn exploitability_decreases_with_iterations() {
    let result_low = solve_push_fold(10.0, 500, 0.0);
    let result_high = solve_push_fold(10.0, 5000, 0.0);

    assert!(
        result_high.exploitability < result_low.exploitability + 0.01,
        "more iterations should reduce exploitability: {} (500 iter) vs {} (5000 iter)",
        result_low.exploitability,
        result_high.exploitability,
    );
}

#[test]
fn exploitability_near_zero() {
    let result = solve_push_fold(10.0, 10000, 0.0);
    assert!(
        result.exploitability < 0.1,
        "exploitability {} should be < 0.1 bb after 10K iterations",
        result.exploitability,
    );
}

// ---------------------------------------------------------------------------
// Strategy validity
// ---------------------------------------------------------------------------

#[test]
fn all_strategies_are_probabilities() {
    let result = solve_push_fold(10.0, 5000, 0.0);
    for i in 0..NUM_HANDS {
        assert!(
            result.push_strategy[i] >= 0.0 && result.push_strategy[i] <= 1.0,
            "push_strategy[{}] ({}) = {} not in [0,1]",
            i,
            bucket_to_hand(i),
            result.push_strategy[i],
        );
        assert!(
            result.call_strategy[i] >= 0.0 && result.call_strategy[i] <= 1.0,
            "call_strategy[{}] ({}) = {} not in [0,1]",
            i,
            bucket_to_hand(i),
            result.call_strategy[i],
        );
    }
}

// ---------------------------------------------------------------------------
// Known push/fold ranges at 10bb
// ---------------------------------------------------------------------------

#[test]
fn premium_hands_always_push() {
    let result = solve_push_fold(10.0, 5000, 0.0);
    let premiums = ["AA", "KK", "QQ", "JJ", "TT", "AKs", "AKo", "AQs"];
    for hand in &premiums {
        let b = hand_to_bucket(hand).unwrap();
        assert!(
            result.push_strategy[b] > 0.9,
            "{} should push >90% at 10bb, got {:.1}%",
            hand,
            result.push_strategy[b] * 100.0,
        );
    }
}

#[test]
fn premium_hands_always_call() {
    let result = solve_push_fold(10.0, 5000, 0.0);
    let premiums = ["AA", "KK", "QQ", "JJ", "TT", "AKs", "AKo"];
    for hand in &premiums {
        let b = hand_to_bucket(hand).unwrap();
        assert!(
            result.call_strategy[b] > 0.9,
            "{} should call >90% at 10bb, got {:.1}%",
            hand,
            result.call_strategy[b] * 100.0,
        );
    }
}

#[test]
fn worst_hands_rarely_push_10bb() {
    let result = solve_push_fold(10.0, 5000, 0.0);
    let trash = ["72o", "83o", "93o", "82o"];
    for hand in &trash {
        let b = hand_to_bucket(hand).unwrap();
        assert!(
            result.push_strategy[b] < 0.3,
            "{} should push <30% at 10bb, got {:.1}%",
            hand,
            result.push_strategy[b] * 100.0,
        );
    }
}

#[test]
fn sb_push_range_reasonable_10bb() {
    // At 10bb, SB should push ~45-65% of hands.
    let result = solve_push_fold(10.0, 5000, 0.0);
    let pct = push_pct(&result);
    assert!(
        pct > 40.0 && pct < 70.0,
        "SB push range {:.1}% should be 40-70% at 10bb",
        pct,
    );
}

#[test]
fn bb_call_range_reasonable_10bb() {
    // At 10bb, BB should call ~25-45% of hands.
    let result = solve_push_fold(10.0, 5000, 0.0);
    let pct = call_pct(&result);
    assert!(
        pct > 20.0 && pct < 50.0,
        "BB call range {:.1}% should be 20-50% at 10bb",
        pct,
    );
}

// ---------------------------------------------------------------------------
// Stack depth effects
// ---------------------------------------------------------------------------

#[test]
fn deeper_stacks_tighter_push() {
    // With more chips at risk, SB should push fewer hands.
    let result_5 = solve_push_fold(5.0, 5000, 0.0);
    let result_20 = solve_push_fold(20.0, 5000, 0.0);

    let pct_5 = push_pct(&result_5);
    let pct_20 = push_pct(&result_20);

    assert!(
        pct_5 > pct_20,
        "push range at 5bb ({:.1}%) should be wider than at 20bb ({:.1}%)",
        pct_5,
        pct_20,
    );
}

#[test]
fn deeper_stacks_tighter_call() {
    let result_5 = solve_push_fold(5.0, 5000, 0.0);
    let result_20 = solve_push_fold(20.0, 5000, 0.0);

    let pct_5 = call_pct(&result_5);
    let pct_20 = call_pct(&result_20);

    assert!(
        pct_5 > pct_20,
        "call range at 5bb ({:.1}%) should be wider than at 20bb ({:.1}%)",
        pct_5,
        pct_20,
    );
}

// ---------------------------------------------------------------------------
// Rake effects
// ---------------------------------------------------------------------------

#[test]
fn rake_tightens_ranges() {
    let no_rake = solve_push_fold(10.0, 5000, 0.0);
    let raked = solve_push_fold(10.0, 5000, 5.0);

    let push_no_rake = push_pct(&no_rake);
    let push_raked = push_pct(&raked);
    let call_no_rake = call_pct(&no_rake);
    let call_raked = call_pct(&raked);

    assert!(
        push_raked <= push_no_rake + 2.0,
        "rake should tighten push range: no_rake={:.1}%, raked={:.1}%",
        push_no_rake,
        push_raked,
    );
    assert!(
        call_raked <= call_no_rake + 2.0,
        "rake should tighten call range: no_rake={:.1}%, raked={:.1}%",
        call_no_rake,
        call_raked,
    );
}

// ---------------------------------------------------------------------------
// Hand bucket mapping
// ---------------------------------------------------------------------------

#[test]
fn all_buckets_roundtrip() {
    for i in 0..169 {
        let hand = bucket_to_hand(i);
        let bucket = hand_to_bucket(&hand).unwrap();
        assert_eq!(bucket, i, "roundtrip failed for {} (bucket {})", hand, i);
    }
}

#[test]
fn known_hand_buckets() {
    assert_eq!(hand_to_bucket("AA"), Some(0));
    assert_eq!(hand_to_bucket("AKs"), Some(1));
    assert_eq!(hand_to_bucket("AKo"), Some(13));
    assert_eq!(hand_to_bucket("KK"), Some(14));
    assert_eq!(hand_to_bucket("22"), Some(168));
}
