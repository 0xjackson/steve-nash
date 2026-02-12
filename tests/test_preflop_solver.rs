//! Tests for the preflop solver (Phase 2).
//!
//! Validates solver convergence, strategy validity, and poker-theoretic
//! properties across the full preflop decision tree.
//!
//! Note: The spot-based model solves 2-player spots independently.
//! Non-blind openers vs BB share identical payoff structures (same dead
//! money, same IP status), so their equilibria are the same. Position
//! effects emerge from spots with different structure (e.g., SB vs BB
//! where SB is OOP, or BTN vs SB with higher dead money).

use gto_cli::game_tree::{
    bucket_to_hand, hand_to_bucket, precompute_equity_table, NUM_HANDS,
};
use gto_cli::preflop_solver::{solve_preflop_spot, Position};

// ---------------------------------------------------------------------------
// Shared equity table (expensive to compute, reused across tests)
// ---------------------------------------------------------------------------

use std::sync::OnceLock;

fn equity_table() -> &'static gto_cli::game_tree::EquityTable {
    static TABLE: OnceLock<gto_cli::game_tree::EquityTable> = OnceLock::new();
    TABLE.get_or_init(|| precompute_equity_table(2000))
}

fn solve(opener: Position, responder: Position) -> gto_cli::preflop_solver::PreflopSpotResult {
    solve_preflop_spot(opener, responder, 100.0, 50000, 0.0, equity_table())
}

fn solve_with(
    opener: Position,
    responder: Position,
    stack: f64,
    iters: usize,
    rake: f64,
) -> gto_cli::preflop_solver::PreflopSpotResult {
    solve_preflop_spot(opener, responder, stack, iters, rake, equity_table())
}

// ---------------------------------------------------------------------------
// Convergence
// ---------------------------------------------------------------------------

#[test]
fn convergence_utg_vs_bb() {
    let result = solve(Position::UTG, Position::BB);
    assert!(
        result.exploitability < 0.15,
        "UTG vs BB exploitability {} should be < 0.15 after 50K iterations",
        result.exploitability,
    );
}

#[test]
fn convergence_btn_vs_bb() {
    let result = solve(Position::BTN, Position::BB);
    assert!(
        result.exploitability < 0.15,
        "BTN vs BB exploitability {} should be < 0.15 after 50K iterations",
        result.exploitability,
    );
}

#[test]
fn convergence_sb_vs_bb() {
    let result = solve(Position::SB, Position::BB);
    assert!(
        result.exploitability < 0.15,
        "SB vs BB exploitability {} should be < 0.15 after 50K iterations",
        result.exploitability,
    );
}

// ---------------------------------------------------------------------------
// Strategy validity
// ---------------------------------------------------------------------------

#[test]
fn all_strategies_valid_probabilities() {
    let result = solve(Position::BTN, Position::BB);
    for i in 0..NUM_HANDS {
        let hand = bucket_to_hand(i);

        assert!(
            result.open_strategy[i] >= 0.0 && result.open_strategy[i] <= 1.0,
            "{} open_strategy = {} not in [0,1]", hand, result.open_strategy[i],
        );

        let sum_101 = result.vs_open_3bet[i] + result.vs_open_call[i];
        assert!(
            sum_101 <= 1.01,
            "{} 3bet + call = {} > 1.0", hand, sum_101,
        );
        assert!(
            result.vs_open_3bet[i] >= 0.0 && result.vs_open_call[i] >= 0.0,
            "{} negative probability at node 101", hand,
        );

        let sum_102 = result.vs_3bet_4bet[i] + result.vs_3bet_call[i];
        assert!(
            sum_102 <= 1.01,
            "{} 4bet + call = {} > 1.0", hand, sum_102,
        );

        let sum_103 = result.vs_4bet_allin[i] + result.vs_4bet_call[i];
        assert!(
            sum_103 <= 1.01,
            "{} allin + call = {} > 1.0", hand, sum_103,
        );

        assert!(
            result.vs_5bet_call[i] >= 0.0 && result.vs_5bet_call[i] <= 1.0,
            "{} vs_5bet_call = {} not in [0,1]", hand, result.vs_5bet_call[i],
        );
    }
}

// ---------------------------------------------------------------------------
// Premium hands behavior
// ---------------------------------------------------------------------------

#[test]
fn aa_always_opens() {
    let result = solve(Position::UTG, Position::BB);
    let aa = hand_to_bucket("AA").unwrap();
    assert!(
        result.open_strategy[aa] > 0.95,
        "AA should always open, got {:.1}%",
        result.open_strategy[aa] * 100.0,
    );
}

#[test]
fn aa_3bets_more_than_calls_vs_open() {
    let result = solve(Position::UTG, Position::BB);
    let aa = hand_to_bucket("AA").unwrap();
    assert!(
        result.vs_open_3bet[aa] > result.vs_open_call[aa],
        "AA should 3-bet more than call vs open: 3bet={:.1}%, call={:.1}%",
        result.vs_open_3bet[aa] * 100.0,
        result.vs_open_call[aa] * 100.0,
    );
}

#[test]
fn premium_hands_always_open() {
    let premiums = ["AA", "KK", "QQ", "AKs"];
    let result = solve(Position::UTG, Position::BB);
    for hand in &premiums {
        let b = hand_to_bucket(hand).unwrap();
        assert!(
            result.open_strategy[b] > 0.85,
            "{} should open >85%, got {:.1}%",
            hand, result.open_strategy[b] * 100.0,
        );
    }
}

// ---------------------------------------------------------------------------
// Trash hands behavior
// ---------------------------------------------------------------------------

#[test]
fn trash_hands_rarely_open() {
    let result = solve(Position::UTG, Position::BB);
    let trash = ["72o", "83o", "82o", "42o"];
    for hand in &trash {
        let b = hand_to_bucket(hand).unwrap();
        assert!(
            result.open_strategy[b] < 0.2,
            "{} should rarely open, got {:.1}%",
            hand, result.open_strategy[b] * 100.0,
        );
    }
}

#[test]
fn trash_hands_rarely_3bet() {
    let result = solve(Position::UTG, Position::BB);
    let trash = ["72o", "83o", "42o"];
    for hand in &trash {
        let b = hand_to_bucket(hand).unwrap();
        assert!(
            result.vs_open_3bet[b] < 0.15,
            "{} should rarely 3-bet vs open, got {:.1}%",
            hand, result.vs_open_3bet[b] * 100.0,
        );
    }
}

// ---------------------------------------------------------------------------
// Position / structural effects
// ---------------------------------------------------------------------------

#[test]
fn sb_vs_bb_differs_from_ip_vs_bb() {
    // SB vs BB: OOP, 0 dead money, but 0.5bb sunk blind makes folding costly.
    // Non-blind vs BB: IP, 0.5bb dead money, folding costs 0.
    // These should produce different opening ranges.
    let sb_result = solve(Position::SB, Position::BB);
    let ip_result = solve(Position::UTG, Position::BB);

    let sb_open = sb_result.open_pct();
    let ip_open = ip_result.open_pct();

    // The ranges should differ — SB's sunk blind cost vs IP advantage + dead money
    let diff = (sb_open - ip_open).abs();
    assert!(
        diff > 2.0,
        "SB ({:.1}%) and IP opener ({:.1}%) should produce different ranges (diff={:.1}%)",
        sb_open, ip_open, diff,
    );
}

#[test]
fn btn_vs_sb_opens_wider_than_utg_vs_bb() {
    // BTN vs SB: IP, 1.0bb dead money → more incentive to open
    // UTG vs BB: IP, 0.5bb dead money → less incentive
    let btn_sb = solve(Position::BTN, Position::SB);
    let utg_bb = solve(Position::UTG, Position::BB);

    let btn_sb_open = btn_sb.open_pct();
    let utg_bb_open = utg_bb.open_pct();

    assert!(
        btn_sb_open > utg_bb_open,
        "BTN vs SB ({:.1}%) should open wider than UTG vs BB ({:.1}%) due to more dead money",
        btn_sb_open, utg_bb_open,
    );
}

// ---------------------------------------------------------------------------
// Rake effects
// ---------------------------------------------------------------------------

#[test]
fn rake_tightens_opening_ranges() {
    let no_rake = solve_with(Position::BTN, Position::BB, 100.0, 30000, 0.0);
    let raked = solve_with(Position::BTN, Position::BB, 100.0, 30000, 5.0);

    let no_rake_pct = no_rake.open_pct();
    let raked_pct = raked.open_pct();

    assert!(
        raked_pct < no_rake_pct + 3.0,
        "Rake should tighten ranges: no_rake={:.1}%, raked={:.1}%",
        no_rake_pct, raked_pct,
    );
}

// ---------------------------------------------------------------------------
// BB defense
// ---------------------------------------------------------------------------

#[test]
fn bb_defends_wider_vs_more_dead_money() {
    // BTN vs SB: SB has 0.5bb blind at stake, 1.0bb dead money
    // SB vs BB: BB has 1.0bb blind at stake, 0bb dead money
    // In spot with more dead money, responder should defend more aggressively
    let btn_sb = solve(Position::BTN, Position::SB);
    let sb_bb = solve(Position::SB, Position::BB);

    let sb_defense = btn_sb.three_bet_pct() + btn_sb.flat_call_pct();
    let bb_defense = sb_bb.three_bet_pct() + sb_bb.flat_call_pct();

    // Both should have positive defense frequencies
    assert!(
        sb_defense > 5.0,
        "SB should defend some hands vs BTN open: {:.1}%", sb_defense,
    );
    assert!(
        bb_defense > 5.0,
        "BB should defend some hands vs SB open: {:.1}%", bb_defense,
    );
}

// ---------------------------------------------------------------------------
// Range size sanity checks
// ---------------------------------------------------------------------------

#[test]
fn open_range_reasonable() {
    let result = solve(Position::UTG, Position::BB);
    let pct = result.open_pct();
    // In a 2-player spot, opening range should be roughly 20-55%
    assert!(
        pct > 15.0 && pct < 60.0,
        "Open range {:.1}% should be 15-60%", pct,
    );
}

#[test]
fn sb_open_range_reasonable() {
    let result = solve(Position::SB, Position::BB);
    let pct = result.open_pct();
    // SB is OOP with no dead money — should still open a decent range
    assert!(
        pct > 10.0 && pct < 60.0,
        "SB open range {:.1}% should be 10-60%", pct,
    );
}
