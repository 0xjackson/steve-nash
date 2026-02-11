//! Comprehensive math & logic audit for gto-cli.
//!
//! Validates every mathematical formula, hand evaluation, equity simulation,
//! strategy heuristic, and hand-strength classification against known-correct
//! poker values.

use gto_cli::cards::{parse_board, parse_card, Card};
use gto_cli::equity::{equity_vs_hand, equity_vs_range};
use gto_cli::hand_evaluator::{compare_hands, evaluate_hand, HandCategory};
use gto_cli::math_engine::*;
use gto_cli::multiway::multiway_range_adjustment;
use gto_cli::play::{classify_hand_strength, estimate_villain_range, run_interactive_session};
use gto_cli::postflop::{analyze_board, cbet_recommendation, street_strategy, Wetness};
use gto_cli::ranges::{blockers_remove, range_from_top_pct, total_combos};
use gto_cli::preflop::get_rfi_range;

fn c(notation: &str) -> Card {
    parse_card(notation).unwrap()
}

// =========================================================================
// Section 1: Math Engine — Formula Verification
// =========================================================================

#[test]
fn audit_pot_odds_textbook_cases() {
    // Harrington on Hold'em: bet / (pot + 2*bet)
    let cases: &[(f64, f64, f64)] = &[
        (100.0, 50.0, 0.25),    // half pot bet
        (100.0, 100.0, 1.0/3.0), // full pot bet
        (50.0, 150.0, 150.0/350.0), // 3x pot overbet
        (200.0, 66.0, 66.0/332.0),  // small bet into big pot
    ];
    for &(pot, bet, expected) in cases {
        let result = pot_odds(pot, bet).unwrap();
        assert!(
            (result - expected).abs() < 0.001,
            "pot_odds({}, {}) = {}, expected {}",
            pot, bet, result, expected
        );
    }
}

#[test]
fn audit_ev_at_break_even_is_zero() {
    // Mathematical identity: EV at break-even equity = 0
    let scenarios: &[(f64, f64)] = &[
        (100.0, 50.0),
        (200.0, 100.0),
        (50.0, 150.0),
        (300.0, 75.0),
        (100.0, 33.0),
        (500.0, 250.0),
        (80.0, 80.0),
        (1000.0, 500.0),
        (60.0, 20.0),
        (150.0, 45.0),
    ];
    for &(pot, bet) in scenarios {
        let equity = pot_odds(pot, bet).unwrap();
        let result = ev(equity, pot, bet);
        assert!(
            result.abs() < 0.01,
            "EV at break-even should be ~0 for pot={}, bet={}, got {}",
            pot, bet, result
        );
    }
}

#[test]
fn audit_ev_positive_with_edge() {
    // ev(0.40, 100, 50) = 0.4 * 150 - 0.6 * 50 = 60 - 30 = 30
    let result = ev(0.40, 100.0, 50.0);
    assert!(
        (result - 30.0).abs() < 0.01,
        "EV should be 30.0, got {}",
        result
    );
}

#[test]
fn audit_ev_negative_when_behind() {
    // ev(0.15, 100, 100) = 0.15 * 200 - 0.85 * 100 = 30 - 85 = -55
    let result = ev(0.15, 100.0, 100.0);
    assert!(
        (result - (-55.0)).abs() < 0.01,
        "EV should be -55.0, got {}",
        result
    );
}

#[test]
fn audit_mdf_textbook_cases() {
    // MDF = pot / (pot + bet)
    let cases: &[(f64, f64, f64)] = &[
        (50.0, 100.0, 2.0/3.0),  // half pot bet
        (100.0, 100.0, 0.5),     // full pot bet
        (200.0, 100.0, 1.0/3.0), // 2x pot overbet
    ];
    for &(bet, pot, expected) in cases {
        let result = mdf(bet, pot).unwrap();
        assert!(
            (result - expected).abs() < 0.001,
            "mdf({}, {}) = {}, expected {}",
            bet, pot, result, expected
        );
    }
}

#[test]
fn audit_bluff_ratio_textbook() {
    // GTO alpha = bet / (pot + bet)
    let cases: &[(f64, f64, f64)] = &[
        (75.0, 100.0, 75.0/175.0),   // 75% pot
        (100.0, 100.0, 0.5),          // full pot
        (50.0, 100.0, 50.0/150.0),    // half pot
    ];
    for &(bet, pot, expected) in cases {
        let result = bluff_to_value_ratio(bet, pot).unwrap();
        assert!(
            (result - expected).abs() < 0.001,
            "bluff_ratio({}, {}) = {}, expected {}",
            bet, pot, result, expected
        );
    }
}

#[test]
fn audit_implied_odds_reduces_needed_equity() {
    // implied_odds < pot_odds for any positive expected_future
    let scenarios: &[(f64, f64, f64)] = &[
        (100.0, 50.0, 50.0),
        (200.0, 100.0, 200.0),
        (50.0, 25.0, 100.0),
        (300.0, 150.0, 50.0),
        (100.0, 100.0, 10.0),
    ];
    for &(pot, bet, future) in scenarios {
        let po = pot_odds(pot, bet).unwrap();
        let imp = implied_odds(pot, bet, future).unwrap();
        assert!(
            imp < po,
            "implied_odds({},{},{}) = {} should be < pot_odds = {}",
            pot, bet, future, imp, po
        );
    }
}

#[test]
fn audit_implied_odds_zero_future_equals_pot_odds() {
    let po = pot_odds(100.0, 50.0).unwrap();
    let imp = implied_odds(100.0, 50.0, 0.0).unwrap();
    assert!(
        (po - imp).abs() < 0.001,
        "With 0 future, implied_odds should equal pot_odds"
    );
}

#[test]
fn audit_spr_zone_boundaries() {
    // SPR ≤ 4.0 → Low, 4.0 < SPR ≤ 10.0 → Medium, SPR > 10.0 → High
    let cases: &[(f64, f64, SprZone)] = &[
        (390.0, 100.0, SprZone::Low),     // 3.9
        (400.0, 100.0, SprZone::Low),     // 4.0 (boundary → Low)
        (410.0, 100.0, SprZone::Medium),  // 4.1
        (1000.0, 100.0, SprZone::Medium), // 10.0 (boundary → Medium)
        (1010.0, 100.0, SprZone::High),   // 10.1
        (2000.0, 100.0, SprZone::High),   // 20.0
    ];
    for &(stack, pot, expected_zone) in cases {
        let result = spr(stack, pot).unwrap();
        assert_eq!(
            result.zone, expected_zone,
            "SPR {:.1} should be {:?}, got {:?}",
            stack / pot, expected_zone, result.zone
        );
    }
}

#[test]
fn audit_fold_equity_crossover() {
    // fold_equity crosses zero at fold_pct = bet/(pot+bet) = alpha
    let pot = 100.0;
    let bet = 75.0;
    let alpha = bet / (pot + bet); // ~0.4286

    // Just below alpha: should be negative
    let below = fold_equity(alpha - 0.01, pot, bet);
    assert!(below < 0.0, "Below alpha should be negative, got {}", below);

    // At alpha: should be ~0
    let at = fold_equity(alpha, pot, bet);
    assert!(at.abs() < 0.5, "At alpha should be ~0, got {}", at);

    // Just above alpha: should be positive
    let above = fold_equity(alpha + 0.01, pot, bet);
    assert!(above > 0.0, "Above alpha should be positive, got {}", above);
}

#[test]
fn audit_break_even_equals_pot_odds() {
    // break_even_pct uses the same formula as pot_odds
    let cases: &[(f64, f64)] = &[
        (100.0, 50.0),
        (200.0, 100.0),
        (50.0, 150.0),
    ];
    for &(pot, bet) in cases {
        let po = pot_odds(pot, bet).unwrap();
        let be = break_even_pct(pot, bet).unwrap();
        assert!(
            (po - be).abs() < 0.001,
            "break_even_pct should equal pot_odds for pot={}, bet={}",
            pot, bet
        );
    }
}

// =========================================================================
// Section 2: Hand Evaluator — Correctness
// =========================================================================

#[test]
fn audit_exhaustive_category_ranking() {
    // Construct one hand per category, verify all 45 pairwise orderings
    let boards_and_hands: Vec<(Vec<Card>, Vec<Card>, HandCategory)> = vec![
        // Royal Flush: As Ks on QsJsTs board
        (vec![c("Qs"), c("Js"), c("Ts"), c("2h"), c("3d")], vec![c("As"), c("Ks")], HandCategory::RoyalFlush),
        // Straight Flush: 9h8h on 7h6h5h board
        (vec![c("7h"), c("6h"), c("5h"), c("2c"), c("3d")], vec![c("9h"), c("8h")], HandCategory::StraightFlush),
        // Four of a Kind: KsKh on KdKc board
        (vec![c("Kd"), c("Kc"), c("5s"), c("2h"), c("3d")], vec![c("Ks"), c("Kh")], HandCategory::FourOfAKind),
        // Full House: AsAh on AdKsKh board
        (vec![c("Ad"), c("Ks"), c("Kh"), c("2c"), c("3d")], vec![c("As"), c("Ah")], HandCategory::FullHouse),
        // Flush: As Ts on 8s5s2s board
        (vec![c("8s"), c("5s"), c("2s"), c("Kd"), c("Qh")], vec![c("As"), c("Ts")], HandCategory::Flush),
        // Straight: 9s8h on 7d6c5s board
        (vec![c("7d"), c("6c"), c("5s"), c("Ah"), c("Kc")], vec![c("9s"), c("8h")], HandCategory::Straight),
        // Three of a Kind: QsQh on Qd7s3h board
        (vec![c("Qd"), c("7s"), c("3h"), c("2c"), c("4d")], vec![c("Qs"), c("Qh")], HandCategory::ThreeOfAKind),
        // Two Pair: AsKh on AdKs5c board
        (vec![c("Ad"), c("Ks"), c("5c"), c("2h"), c("3d")], vec![c("As"), c("Kh")], HandCategory::TwoPair),
        // One Pair: AsAh on Kd7s3c board
        (vec![c("Kd"), c("7s"), c("3c"), c("2h"), c("5d")], vec![c("As"), c("Ah")], HandCategory::OnePair),
        // High Card: AhKs on Qd9c3s board
        (vec![c("Qd"), c("9c"), c("3s"), c("2h"), c("5d")], vec![c("Ah"), c("Ks")], HandCategory::HighCard),
    ];

    // Verify each hand evaluates to expected category
    for (board, hole, expected_cat) in &boards_and_hands {
        let result = evaluate_hand(hole, board).unwrap();
        assert_eq!(
            result.category, *expected_cat,
            "Expected {:?}, got {:?}",
            expected_cat, result.category
        );
    }

    // Verify pairwise ordering: earlier in list = stronger hand
    for i in 0..boards_and_hands.len() {
        for j in (i + 1)..boards_and_hands.len() {
            let r_i = evaluate_hand(&boards_and_hands[i].1, &boards_and_hands[i].0).unwrap();
            let r_j = evaluate_hand(&boards_and_hands[j].1, &boards_and_hands[j].0).unwrap();
            assert!(
                r_i > r_j,
                "{:?} (rank {}) should beat {:?} (rank {})",
                boards_and_hands[i].2,
                r_i.rank,
                boards_and_hands[j].2,
                r_j.rank
            );
        }
    }
}

#[test]
fn audit_kicker_resolution_one_pair() {
    // AA-K vs AA-Q: pair of aces, kicker K vs Q
    let board = vec![c("Ad"), c("7s"), c("3c"), c("2h"), c("5d")];
    let r = compare_hands(&[c("As"), c("Kh")], &[c("Ah"), c("Qd")], &board).unwrap();
    assert_eq!(r, 1, "AA with K kicker should beat AA with Q kicker");
}

#[test]
fn audit_kicker_resolution_two_pair() {
    // KK77A vs KK77Q
    let board = vec![c("Ks"), c("7d"), c("7c"), c("2h"), c("3d")];
    let r = compare_hands(&[c("Kh"), c("Ad")], &[c("Kc"), c("Qd")], &board).unwrap();
    assert_eq!(r, 1, "KK77A should beat KK77Q");
}

#[test]
fn audit_kicker_resolution_trips() {
    // AAA-KQ vs AAA-KJ: second kicker Q vs J
    let board = vec![c("As"), c("Ah"), c("Ad"), c("Kc"), c("2s")];
    let r = compare_hands(&[c("Qh"), c("3d")], &[c("Jh"), c("4d")], &board).unwrap();
    assert_eq!(r, 1, "Trips with Q kicker should beat trips with J kicker");
}

#[test]
fn audit_kicker_resolution_full_house() {
    // AAKK vs AAQQ: full house pair rank K vs Q
    let board = vec![c("As"), c("Ah"), c("Ad"), c("2c"), c("3d")];
    let r = compare_hands(&[c("Ks"), c("Kh")], &[c("Qs"), c("Qh")], &board).unwrap();
    assert_eq!(r, 1, "AAA-KK should beat AAA-QQ");
}

#[test]
fn audit_wheel_below_six_high_straight() {
    // Wheel (A-2-3-4-5) ranks below 6-high straight (2-3-4-5-6)
    let board_wheel = vec![c("As"), c("2d"), c("3c"), c("4h"), c("5s")];
    let board_six = vec![c("2s"), c("3d"), c("4c"), c("5h"), c("6s")];

    let wheel = evaluate_hand(&[c("7h"), c("8d")], &board_wheel).unwrap();
    let six_high = evaluate_hand(&[c("7h"), c("8d")], &board_six).unwrap();

    assert_eq!(wheel.category, HandCategory::Straight);
    assert_eq!(six_high.category, HandCategory::Straight);
    assert!(
        six_high > wheel,
        "6-high straight should beat wheel"
    );
}

#[test]
fn audit_ace_high_vs_king_high_flush() {
    // 3 spades on board + 1 each in hero/villain = 5-card flush each
    let board = vec![c("Ts"), c("7s"), c("4s"), c("2d"), c("3d")];
    let r = compare_hands(&[c("As"), c("9s")], &[c("Ks"), c("8s")], &board).unwrap();
    assert_eq!(r, 1, "Ace-high flush should beat King-high flush");
}

#[test]
fn audit_board_plays_tie() {
    // Both players have same best 5 cards from the board
    let board = vec![c("As"), c("Kd"), c("Qh"), c("Js"), c("Ts")];
    let r = compare_hands(&[c("2h"), c("3d")], &[c("4h"), c("5d")], &board).unwrap();
    assert_eq!(r, 0, "Board plays should be a tie");
}

#[test]
fn audit_7_card_evaluation_finds_optimal_5() {
    // Hero has 7h 8h, board has 6h 5h 4h Ac Kd
    // Best 5 = straight flush (4h-5h-6h-7h-8h), not just the straight
    let hole = vec![c("7h"), c("8h")];
    let board = vec![c("6h"), c("5h"), c("4h"), c("Ac"), c("Kd")];
    let result = evaluate_hand(&hole, &board).unwrap();
    assert_eq!(
        result.category,
        HandCategory::StraightFlush,
        "Should find straight flush from 7 cards"
    );
}

// =========================================================================
// Section 3: Equity Calculator — Statistical Validation
// =========================================================================

#[test]
fn audit_aa_vs_kk_preflop() {
    // Known: AA vs KK ~82% equity (PokerStove)
    let result = equity_vs_hand(
        &[c("As"), c("Ah")],
        &[c("Ks"), c("Kh")],
        None,
        50000,
    ).unwrap();
    let eq = result.equity();
    assert!(
        (eq - 0.82).abs() < 0.03,
        "AA vs KK should be ~82%, got {:.1}%",
        eq * 100.0
    );
}

#[test]
fn audit_aks_vs_qq_preflop() {
    // Known: AKs vs QQ ~46% equity (PokerStove)
    let result = equity_vs_hand(
        &[c("As"), c("Ks")],
        &[c("Qh"), c("Qd")],
        None,
        50000,
    ).unwrap();
    let eq = result.equity();
    assert!(
        (eq - 0.46).abs() < 0.03,
        "AKs vs QQ should be ~46%, got {:.1}%",
        eq * 100.0
    );
}

#[test]
fn audit_ako_vs_22_preflop() {
    // Known: AKo vs 22 ~47% (PokerStove — classic coinflip)
    let result = equity_vs_hand(
        &[c("As"), c("Kh")],
        &[c("2s"), c("2d")],
        None,
        50000,
    ).unwrap();
    let eq = result.equity();
    assert!(
        (eq - 0.47).abs() < 0.04,
        "AKo vs 22 should be ~47%, got {:.1}%",
        eq * 100.0
    );
}

#[test]
fn audit_set_vs_flush_draw_on_flop() {
    // Set of tens vs nut flush draw on Ts 9s 2h: set ~65-70%
    let board = parse_board("Ts9s2h").unwrap();
    let result = equity_vs_hand(
        &[c("Td"), c("Th")],
        &[c("As"), c("Ks")],
        Some(&board),
        50000,
    ).unwrap();
    let eq = result.equity();
    assert!(
        eq > 0.55 && eq < 0.80,
        "Set vs flush draw should be ~65-70%, got {:.1}%",
        eq * 100.0
    );
}

#[test]
fn audit_overpair_vs_oesd_on_flop() {
    // AA vs JT on T86 rainbow: AA is overpair, JT has OESD (needs 7 or 9)
    // AA should be ~70-75% favorite
    let board = parse_board("Td8c6h").unwrap();
    let result = equity_vs_hand(
        &[c("As"), c("Ah")],
        &[c("Jd"), c("9s")],
        Some(&board),
        50000,
    ).unwrap();
    let eq = result.equity();
    assert!(
        eq > 0.50 && eq < 0.85,
        "Overpair vs OESD should be ~65-75%, got {:.1}%",
        eq * 100.0
    );
}

#[test]
fn audit_equity_symmetry() {
    // equity(hand1 vs hand2) + equity(hand2 vs hand1) + ties ≈ 1.0
    let result1 = equity_vs_hand(
        &[c("As"), c("Kh")],
        &[c("Qd"), c("Qc")],
        None,
        50000,
    ).unwrap();

    // hand1's equity = win + tie/2, hand2's equity = lose + tie/2
    // Together: win + lose + tie = 1.0
    let sum = result1.win + result1.lose + result1.tie;
    assert!(
        (sum - 1.0).abs() < 0.001,
        "win + lose + tie should sum to 1.0, got {}",
        sum
    );
}

#[test]
fn audit_equity_vs_top_5pct_range() {
    // AA vs top 5% range (AA,KK,QQ,AKs,JJ)
    let range = range_from_top_pct(5.0).unwrap();
    let result = equity_vs_range(
        &[c("As"), c("Ah")],
        &range,
        None,
        10000,
    ).unwrap();
    let eq = result.equity();
    // AA dominates most of top 5% except mirror
    assert!(
        eq > 0.60 && eq < 0.95,
        "AA vs top 5% should be ~75-85%, got {:.1}%",
        eq * 100.0
    );
}

// =========================================================================
// Section 4: Board Texture — Classification Audit
// =========================================================================

#[test]
fn audit_board_texture_dry_rainbow_disconnected() {
    // Ks 7d 2c: dry, rainbow, disconnected
    let board = parse_board("Ks7d2c").unwrap();
    let tex = analyze_board(&board).unwrap();
    assert_eq!(tex.wetness, Wetness::Dry, "K72r should be dry");
    assert!(tex.is_rainbow, "K72r should be rainbow");
    assert!(!tex.is_paired, "K72r should not be paired");
}

#[test]
fn audit_board_texture_wet_two_tone_connected() {
    // Ts 9s 8d: wet, two-tone, connected
    let board = parse_board("Ts9s8d").unwrap();
    let tex = analyze_board(&board).unwrap();
    assert_eq!(tex.wetness, Wetness::Wet, "T98 two-tone should be wet");
    assert!(tex.is_two_tone, "T98ss should be two-tone");
    assert!(tex.straight_draw_possible, "T98 should have straight draws");
}

#[test]
fn audit_board_texture_monotone() {
    // As Ks Qs: wet, monotone
    let board = parse_board("AsKsQs").unwrap();
    let tex = analyze_board(&board).unwrap();
    assert_eq!(tex.wetness, Wetness::Wet, "AKQ monotone should be wet");
    assert!(tex.is_monotone, "AKQ all spades should be monotone");
}

#[test]
fn audit_board_texture_paired_dry() {
    // 7s 7d 2c: dry (paired reduces wetness), paired
    let board = parse_board("7s7d2c").unwrap();
    let tex = analyze_board(&board).unwrap();
    assert!(tex.is_paired, "77x should be paired");
    // Paired boards score -1, two-tone scores +1 → score 0 → Dry
    assert_eq!(tex.wetness, Wetness::Dry, "Paired 772 rainbow should be dry");
}

#[test]
fn audit_board_texture_connected_wet() {
    // Js Th 9s: wet, two-tone + connected = score 3 → Wet
    let board = parse_board("JsTh9s").unwrap();
    let tex = analyze_board(&board).unwrap();
    assert_eq!(tex.wetness, Wetness::Wet, "JT9 two-tone connected should be wet");
    assert!(tex.straight_draw_possible, "JT9 should have straight draws");
    assert!(tex.is_two_tone, "JT9ss should be two-tone");
}

#[test]
fn audit_board_texture_low_dry() {
    // 2s 5h 9d: dry, rainbow, disconnected
    let board = parse_board("2s5h9d").unwrap();
    let tex = analyze_board(&board).unwrap();
    assert_eq!(tex.wetness, Wetness::Dry, "259r should be dry");
    assert!(tex.is_rainbow, "259r should be rainbow");
}

#[test]
fn audit_board_texture_turn_monotone() {
    // Turn: Qs Js Ts 2c — monotone detection on 4-card board
    let board = parse_board("QsJsTs2c").unwrap();
    let tex = analyze_board(&board).unwrap();
    // First three are monotone, max suit count = 3
    assert!(tex.is_monotone, "QJT all spades + 2c should be monotone");
    assert_eq!(tex.wetness, Wetness::Wet, "Monotone connected should be wet");
}

// =========================================================================
// Section 5: Hand Strength Classifier — Specific Scenarios
// =========================================================================

#[test]
fn audit_classify_royal_flush() {
    let hole = vec![c("As"), c("Ks")];
    let board = parse_board("QsJsTs").unwrap();
    let result = evaluate_hand(&hole, &board).unwrap();
    let strength = classify_hand_strength(&result, &hole, &board, 0.99);
    assert_eq!(strength, "nuts");
}

#[test]
fn audit_classify_set() {
    let hole = vec![c("7s"), c("7h")];
    let board = parse_board("7dKs2c").unwrap();
    let result = evaluate_hand(&hole, &board).unwrap();
    let strength = classify_hand_strength(&result, &hole, &board, 0.85);
    assert_eq!(strength, "very_strong", "Set should be very_strong");
}

#[test]
fn audit_classify_trips_not_set() {
    // 7h 6s on 7d 7c Ks board: trips (not pocket pair), should be "strong"
    let hole = vec![c("7h"), c("6s")];
    let board = parse_board("7d7cKs").unwrap();
    let result = evaluate_hand(&hole, &board).unwrap();
    let strength = classify_hand_strength(&result, &hole, &board, 0.70);
    assert_eq!(strength, "strong", "Trips (not set) should be strong");
}

#[test]
fn audit_classify_top_pair_ace_kicker() {
    let hole = vec![c("Ah"), c("Ks")];
    let board = parse_board("Kd7c2h").unwrap();
    let result = evaluate_hand(&hole, &board).unwrap();
    let strength = classify_hand_strength(&result, &hole, &board, 0.72);
    assert_eq!(strength, "strong", "Top pair + A kicker should be strong");
}

#[test]
fn audit_classify_top_pair_weak_kicker() {
    let hole = vec![c("Kh"), c("4s")];
    let board = parse_board("Kd7c2h").unwrap();
    let result = evaluate_hand(&hole, &board).unwrap();
    let strength = classify_hand_strength(&result, &hole, &board, 0.55);
    assert_eq!(strength, "medium", "Top pair weak kicker should be medium");
}

#[test]
fn audit_classify_weak_high_card_no_draw() {
    let hole = vec![c("9h"), c("8c")];
    let board = parse_board("AdKc2h").unwrap();
    let result = evaluate_hand(&hole, &board).unwrap();
    let strength = classify_hand_strength(&result, &hole, &board, 0.15);
    assert_eq!(strength, "weak", "Whiffed high card should be weak");
}

#[test]
fn audit_classify_flush_draw() {
    // As Ts on Ks 7s 2d: high card + flush draw → "draw"
    let hole = vec![c("As"), c("Ts")];
    let board = parse_board("Ks7s2d").unwrap();
    let result = evaluate_hand(&hole, &board).unwrap();
    // The hand evaluates as high card (no made hand yet)
    // But hero has 2 spades + 2 on board = 4 spades = flush draw
    let strength = classify_hand_strength(&result, &hole, &board, 0.30);
    assert_eq!(strength, "draw", "Flush draw should classify as draw");
}

#[test]
fn audit_classify_oesd() {
    // Jh Tc on Ks Qd 2h: OESD (needs A or 9) → "draw"
    let hole = vec![c("Jh"), c("Tc")];
    let board = parse_board("KsQd2h").unwrap();
    let result = evaluate_hand(&hole, &board).unwrap();
    let strength = classify_hand_strength(&result, &hole, &board, 0.30);
    assert_eq!(strength, "draw", "OESD should classify as draw");
}

#[test]
fn audit_classify_full_house() {
    let hole = vec![c("5s"), c("5h")];
    let board = parse_board("5d3c3h").unwrap();
    let result = evaluate_hand(&hole, &board).unwrap();
    let strength = classify_hand_strength(&result, &hole, &board, 0.95);
    assert_eq!(strength, "very_strong", "Full house should be very_strong");
}

#[test]
fn audit_classify_quads() {
    let hole = vec![c("As"), c("Ah")];
    let board = parse_board("AdAc2h").unwrap();
    let result = evaluate_hand(&hole, &board).unwrap();
    let strength = classify_hand_strength(&result, &hole, &board, 0.99);
    assert_eq!(strength, "nuts", "Quads should be nuts");
}

#[test]
fn audit_classify_straight_flush() {
    let hole = vec![c("9s"), c("8s")];
    let board = parse_board("7s6s5s").unwrap();
    let result = evaluate_hand(&hole, &board).unwrap();
    let strength = classify_hand_strength(&result, &hole, &board, 0.99);
    assert_eq!(strength, "nuts", "Straight flush should be nuts");
}

#[test]
fn audit_classify_strong_flush() {
    // Nut flush with high equity → very_strong
    let hole = vec![c("As"), c("Ts")];
    let board = parse_board("Ks7s2s").unwrap();
    let result = evaluate_hand(&hole, &board).unwrap();
    let strength = classify_hand_strength(&result, &hole, &board, 0.80);
    assert_eq!(strength, "very_strong", "Nut flush with high equity should be very_strong");
}

#[test]
fn audit_classify_weak_straight() {
    // Low straight with low equity → strong
    let hole = vec![c("5s"), c("4h")];
    let board = parse_board("6d7c8s").unwrap();
    let result = evaluate_hand(&hole, &board).unwrap();
    let strength = classify_hand_strength(&result, &hole, &board, 0.55);
    assert_eq!(strength, "strong", "Low straight with lower equity should be strong");
}

#[test]
fn audit_classify_two_pair_strong() {
    let hole = vec![c("Kh"), c("7s")];
    let board = parse_board("Kd7c2h").unwrap();
    let result = evaluate_hand(&hole, &board).unwrap();
    let strength = classify_hand_strength(&result, &hole, &board, 0.75);
    assert_eq!(strength, "strong", "Two pair with good equity should be strong");
}

#[test]
fn audit_classify_second_pair() {
    // Pair the second board card: medium or weak
    let hole = vec![c("7h"), c("6s")];
    let board = parse_board("Kd7c2h").unwrap();
    let result = evaluate_hand(&hole, &board).unwrap();
    let strength = classify_hand_strength(&result, &hole, &board, 0.45);
    assert_eq!(strength, "medium", "Second pair should be medium");
}

#[test]
fn audit_classify_bottom_pair() {
    let hole = vec![c("2s"), c("6h")];
    let board = parse_board("Kd7c2h").unwrap();
    let result = evaluate_hand(&hole, &board).unwrap();
    let strength = classify_hand_strength(&result, &hole, &board, 0.30);
    assert_eq!(strength, "weak", "Bottom pair should be weak");
}

#[test]
fn audit_classify_strength_always_valid_for_strategy() {
    // Every possible classify output must be accepted by street_strategy without panic
    let valid_strengths = ["nuts", "very_strong", "strong", "medium", "draw", "weak", "bluff"];
    let board_cards = parse_board("Ks7d2c").unwrap();
    let texture = analyze_board(&board_cards).unwrap();

    for &strength in &valid_strengths {
        let strat = street_strategy(strength, &texture, 100.0, 200.0, "IP", "flop");
        assert!(!strat.action.is_empty(), "Strategy action should not be empty for {}", strength);
    }
}

// =========================================================================
// Section 6: Villain Range Estimator — Sanity Checks
// =========================================================================

#[test]
fn audit_rfi_wider_than_3bet_range() {
    let hero = vec![c("2h"), c("3d")]; // unrelated blockers
    let rfi = estimate_villain_range("RFI", "BTN", None, &hero, "6max");
    let vs_3bet = estimate_villain_range("vs_3bet", "BTN", Some("CO"), &hero, "6max");
    assert!(
        rfi.len() >= vs_3bet.len(),
        "RFI range ({}) should be >= 3-bet range ({})",
        rfi.len(),
        vs_3bet.len()
    );
}

#[test]
fn audit_blocker_removal_reduces_or_keeps_range() {
    let hero = vec![c("As"), c("Kh")]; // blocks AA, KK, AK combos
    let full_range = range_from_top_pct(20.0).unwrap();
    let blocked = blockers_remove(&full_range, &hero);
    assert!(
        blocked.len() <= full_range.len(),
        "Blocked range ({}) should be ≤ full range ({})",
        blocked.len(),
        full_range.len()
    );
}

#[test]
fn audit_ranges_non_empty_for_all_situations() {
    let hero = vec![c("2h"), c("3d")];
    let situations = ["RFI", "vs_RFI", "vs_3bet", "bb_defense"];
    for &sit in &situations {
        let range = estimate_villain_range(sit, "BTN", Some("UTG"), &hero, "6max");
        assert!(
            !range.is_empty(),
            "Range for situation '{}' should not be empty",
            sit
        );
    }
}

#[test]
fn audit_vs_rfi_returns_villain_rfi_range() {
    // When villain is UTG, vs_RFI should base range on UTG's actual RFI range
    let hero = vec![c("2h"), c("3d")];
    let rfi_range = get_rfi_range("UTG", "6max");
    let vs_rfi = estimate_villain_range("vs_RFI", "CO", Some("UTG"), &hero, "6max");

    // The villain range should be based on UTG's RFI (potentially with blockers removed)
    if !rfi_range.is_empty() {
        // At least some hands from UTG's RFI should appear
        let overlap = vs_rfi.iter().filter(|h| rfi_range.contains(h)).count();
        assert!(
            overlap > 0,
            "vs_RFI should contain hands from villain's actual RFI range"
        );
    }
}

#[test]
fn audit_range_sizes_reasonable() {
    // RFI caller ~20% → 200-300 combos
    let rfi_range = range_from_top_pct(20.0).unwrap();
    let rfi_combos = total_combos(&rfi_range);
    assert!(
        rfi_combos >= 200 && rfi_combos <= 350,
        "Top 20% should be ~200-300 combos, got {}",
        rfi_combos
    );

    // 3-bet range ~7% → 60-100 combos
    let three_bet_range = range_from_top_pct(7.0).unwrap();
    let three_bet_combos = total_combos(&three_bet_range);
    assert!(
        three_bet_combos >= 50 && three_bet_combos <= 120,
        "Top 7% should be ~60-100 combos, got {}",
        three_bet_combos
    );
}

// =========================================================================
// Section 7: Strategy Coherence — Cross-Function Consistency
// =========================================================================

#[test]
fn audit_stronger_hand_more_aggressive_action() {
    let board_cards = parse_board("Ks7d2c").unwrap();
    let texture = analyze_board(&board_cards).unwrap();

    // nuts → BET, weak → CHECK/FOLD
    let nuts_strat = street_strategy("nuts", &texture, 100.0, 200.0, "IP", "flop");
    let weak_strat = street_strategy("weak", &texture, 100.0, 200.0, "IP", "flop");

    assert!(
        nuts_strat.action.contains("BET"),
        "Nuts should BET, got {}",
        nuts_strat.action
    );
    assert!(
        weak_strat.action.contains("CHECK") || weak_strat.action.contains("FOLD"),
        "Weak should CHECK/FOLD, got {}",
        weak_strat.action
    );
}

#[test]
fn audit_ip_plays_wider_than_oop() {
    let board_cards = parse_board("Ks7d2c").unwrap();
    let texture = analyze_board(&board_cards).unwrap();

    // Medium hand: IP gets CHECK/BET, OOP gets CHECK
    let ip_strat = street_strategy("medium", &texture, 100.0, 200.0, "IP", "flop");
    let oop_strat = street_strategy("medium", &texture, 100.0, 200.0, "OOP", "flop");

    assert!(
        ip_strat.action.contains("BET") || ip_strat.action.contains("CHECK/BET"),
        "IP medium should have betting option, got {}",
        ip_strat.action
    );
    assert_eq!(
        oop_strat.action, "CHECK",
        "OOP medium should CHECK, got {}",
        oop_strat.action
    );
}

#[test]
fn audit_low_spr_more_commitment() {
    let board_cards = parse_board("Ks7d2c").unwrap();
    let texture = analyze_board(&board_cards).unwrap();

    // Low SPR (stack=200, pot=100, SPR=2) with strong hand → BET
    let low_spr = street_strategy("very_strong", &texture, 100.0, 200.0, "IP", "flop");
    assert!(
        low_spr.action.contains("BET"),
        "Very strong hand at low SPR should BET, got {}",
        low_spr.action
    );
    // Should mention all-in or stacks at low SPR
    assert!(
        low_spr.sizing.contains("all-in") || low_spr.reasoning.contains("Low SPR"),
        "Low SPR should mention stacking, got sizing='{}', reasoning='{}'",
        low_spr.sizing,
        low_spr.reasoning
    );
}

#[test]
fn audit_river_bluff_mentions_fold_equity() {
    let board_cards = parse_board("Ks7d2cJh3s").unwrap();
    let texture = analyze_board(&board_cards).unwrap();

    let bluff_strat = street_strategy("bluff", &texture, 100.0, 200.0, "IP", "river");
    assert!(
        bluff_strat.reasoning.contains("fold equity") || bluff_strat.reasoning.contains("fold"),
        "River bluff reasoning should mention fold equity, got '{}'",
        bluff_strat.reasoning
    );
}

#[test]
fn audit_multiway_tightens() {
    let adj = multiway_range_adjustment(4);
    assert!(
        adj.to_lowercase().contains("tighten"),
        "4-way adjustment should mention tightening, got '{}'",
        adj
    );
}

#[test]
fn audit_cbet_uses_spr() {
    let board = parse_board("Ks7d2c").unwrap();
    let texture = analyze_board(&board).unwrap();

    // Low SPR should have higher frequency than high SPR
    let low_spr = cbet_recommendation(&texture, "IP", 3.0, false);
    let high_spr = cbet_recommendation(&texture, "IP", 12.0, false);

    assert!(
        low_spr.frequency > high_spr.frequency,
        "Low SPR c-bet freq ({}) should be > high SPR freq ({})",
        low_spr.frequency,
        high_spr.frequency
    );
}

#[test]
fn audit_cbet_spr_adjustment_magnitude() {
    let board = parse_board("Ks7d2c").unwrap();
    let texture = analyze_board(&board).unwrap();

    // Normal SPR (6.0) should have base frequency
    let normal = cbet_recommendation(&texture, "IP", 6.0, false);
    let low = cbet_recommendation(&texture, "IP", 3.0, false);
    let high = cbet_recommendation(&texture, "IP", 12.0, false);

    // Low SPR should bump ~+12%
    assert!(
        (low.frequency - normal.frequency - 0.12).abs() < 0.01,
        "Low SPR should add ~12%: normal={}, low={}",
        normal.frequency,
        low.frequency
    );

    // High SPR should reduce ~-10%
    assert!(
        (normal.frequency - high.frequency - 0.10).abs() < 0.01,
        "High SPR should reduce ~10%: normal={}, high={}",
        normal.frequency,
        high.frequency
    );
}

// =========================================================================
// Section 8: Full Pipeline Smoke Tests (Simulated Sessions)
// =========================================================================

#[test]
fn audit_pipeline_value_hand() {
    // BTN, AhKs, no raise → RAISE, flop Kd7c2h → BET (top pair top kicker)
    let input = b"6max\n1/2\n200\nBTN\n2\nAhKs\nn\ny\nKd7c2h\nbet\n50\ny\nJh\nbet\n75\ny\n3c\nbet\n100\nn\n";
    let mut reader = &input[..];
    let mut output = Vec::new();
    run_interactive_session(&mut reader, &mut output);
    let out = String::from_utf8(output).unwrap();

    assert!(out.contains("RAISE"), "AKs BTN should RAISE preflop");
    assert!(out.contains("Flop") || out.contains("flop"), "Should show flop");
    assert!(out.contains("BET"), "Should recommend BET with top pair");
}

#[test]
fn audit_pipeline_drawing_hand() {
    // CO, Ts9s, no raise → RAISE, flop 8s7d2s (OESD + flush draw)
    let input = b"6max\n1/2\n200\nCO\n2\nTs9s\nn\ny\n8s7d2s\nbet\n50\ny\nAc\ncheck\ny\nKs\nbet\n100\nn\n";
    let mut reader = &input[..];
    let mut output = Vec::new();
    run_interactive_session(&mut reader, &mut output);
    let out = String::from_utf8(output).unwrap();

    assert!(out.contains("RAISE") || out.contains("CALL"), "T9s CO should open");
    // On the flop with draws, should see draw-related advice
    assert!(
        out.contains("draw") || out.contains("Draw") || out.contains("semi-bluff") || out.contains("BET"),
        "Should recognize the drawing hand"
    );
}

#[test]
fn audit_pipeline_missed_hand_fold() {
    // BTN, 7h2c (trash), no raise → likely FOLD preflop
    let input = b"6max\n1/2\n200\nBTN\n2\n7h2c\nn\nn\n";
    let mut reader = &input[..];
    let mut output = Vec::new();
    run_interactive_session(&mut reader, &mut output);
    let out = String::from_utf8(output).unwrap();

    assert!(out.contains("FOLD"), "72o from BTN should FOLD");
}

#[test]
fn audit_pipeline_facing_raise() {
    // CO, QsQh, UTG raised → should CALL or 3BET
    let input = b"6max\n1/2\n200\nCO\n2\nQsQh\ny\nUTG\nn\ny\nAs7d2c\nbet\n50\nn\nn\n";
    let mut reader = &input[..];
    let mut output = Vec::new();
    run_interactive_session(&mut reader, &mut output);
    let out = String::from_utf8(output).unwrap();

    assert!(
        out.contains("CALL") || out.contains("3BET"),
        "QQ vs UTG open should CALL or 3BET, output: {}",
        &out[..out.len().min(500)]
    );
}

#[test]
fn audit_pipeline_bb_defense() {
    // BB, suited connector (8s7s), BTN raised → CALL/3BET/FOLD
    let input = b"6max\n1/2\n200\nBB\n2\n8s7s\ny\nBTN\nn\nn\n";
    let mut reader = &input[..];
    let mut output = Vec::new();
    run_interactive_session(&mut reader, &mut output);
    let out = String::from_utf8(output).unwrap();

    // Should get a valid preflop action
    assert!(
        out.contains("CALL") || out.contains("3BET") || out.contains("FOLD") || out.contains("RAISE"),
        "BB defense should produce a valid action"
    );
    assert!(!out.contains("panic"), "Should not panic");
}

#[test]
fn audit_pipeline_no_panics_full_hand() {
    // Play a complete hand through all streets
    let input = b"6max\n1/2\n200\nBTN\n2\nAsKh\nn\ny\nQd9c3s\ncheck\ny\n5d\ncheck\ny\n2h\ncheck\nn\n";
    let mut reader = &input[..];
    let mut output = Vec::new();
    run_interactive_session(&mut reader, &mut output);
    let out = String::from_utf8(output).unwrap();

    // Verify the session completed without errors
    assert!(out.contains("Welcome to GTO Play!"), "Should show welcome");
    assert!(out.contains("Flop") || out.contains("flop"), "Should reach flop");
    assert!(out.contains("Turn") || out.contains("turn"), "Should reach turn");
    assert!(out.contains("River") || out.contains("river"), "Should reach river");
    assert!(out.contains("Hand Complete"), "Should complete the hand");
}

// =========================================================================
// Section 9: Bug Fix Verification
// =========================================================================

#[test]
fn audit_fix_has_straight_draw_hero_ace_low_no_false_positive() {
    // Board has A-2-3-5 (ace-low draw potential), hero has Kh Qd (unrelated)
    // Hero should NOT have a straight draw — board has all the low cards
    let hole = vec![c("Kh"), c("Qd")];
    let board = parse_board("As2d3c5h").unwrap();
    let result = evaluate_hand(&hole, &board).unwrap();

    // Should NOT classify as draw if hero doesn't contribute
    let strength = classify_hand_strength(&result, &hole, &board, 0.15);
    assert_ne!(
        strength, "draw",
        "KQ on A235 board should NOT be classified as draw (hero doesn't contribute to ace-low)"
    );
}

#[test]
fn audit_fix_has_straight_draw_hero_ace_low_true_positive() {
    // Hero has Ah 4s, board has 2d 3c Ks: hero contributes A and 4 to ace-low draw
    let hole = vec![c("Ah"), c("4s")];
    let board = parse_board("2d3cKs").unwrap();
    let result = evaluate_hand(&hole, &board).unwrap();
    let strength = classify_hand_strength(&result, &hole, &board, 0.30);
    assert_eq!(
        strength, "draw",
        "A4 on 23K should be draw (hero contributes to ace-low straight draw)"
    );
}

#[test]
fn audit_fix_cbet_spr_not_ignored() {
    let board = parse_board("Ks7d2c").unwrap();
    let texture = analyze_board(&board).unwrap();

    let spr3 = cbet_recommendation(&texture, "IP", 3.0, false);
    let spr7 = cbet_recommendation(&texture, "IP", 7.0, false);
    let spr15 = cbet_recommendation(&texture, "IP", 15.0, false);

    // Different SPRs should produce different frequencies
    assert!(
        spr3.frequency != spr7.frequency || spr7.frequency != spr15.frequency,
        "SPR should affect c-bet frequency: spr3={}, spr7={}, spr15={}",
        spr3.frequency, spr7.frequency, spr15.frequency
    );

    // Low SPR → highest frequency
    assert!(
        spr3.frequency > spr7.frequency,
        "SPR 3 ({}) should have higher freq than SPR 7 ({})",
        spr3.frequency, spr7.frequency
    );
    assert!(
        spr7.frequency > spr15.frequency,
        "SPR 7 ({}) should have higher freq than SPR 15 ({})",
        spr7.frequency, spr15.frequency
    );
}
