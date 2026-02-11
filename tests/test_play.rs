use gto_cli::cards::{Card, Rank, Suit};
use gto_cli::hand_evaluator::evaluate_hand;
use gto_cli::math_engine::SprZone;
use gto_cli::play::*;
use gto_cli::postflop::Wetness;
use gto_cli::hand_evaluator::HandCategory;

fn card(rank: Rank, suit: Suit) -> Card {
    Card::new(rank, suit)
}

// ---------------------------------------------------------------------------
// Position logic
// ---------------------------------------------------------------------------

#[test]
fn test_btn_is_ip_vs_all() {
    for &vp in &["SB", "BB", "UTG", "HJ", "CO"] {
        assert!(
            is_in_position("BTN", vp, "6max"),
            "BTN should be IP vs {}",
            vp
        );
    }
}

#[test]
fn test_sb_is_oop_vs_all() {
    for &vp in &["BB", "UTG", "HJ", "CO", "BTN"] {
        assert!(
            !is_in_position("SB", vp, "6max"),
            "SB should be OOP vs {}",
            vp
        );
    }
}

#[test]
fn test_position_order_9max() {
    assert!(is_in_position("BTN", "UTG2", "9max"));
    assert!(is_in_position("CO", "MP", "9max"));
    assert!(!is_in_position("UTG1", "HJ", "9max"));
}

#[test]
fn test_explain_position_all_6max() {
    let positions = ["UTG", "HJ", "CO", "BTN", "SB", "BB"];
    for pos in &positions {
        let explanation = explain_position(pos);
        assert!(!explanation.is_empty(), "No explanation for {}", pos);
    }
}

// ---------------------------------------------------------------------------
// Hand strength classifier
// ---------------------------------------------------------------------------

#[test]
fn test_classify_royal_flush_is_nuts() {
    let hole = vec![
        card(Rank::Ace, Suit::Spades),
        card(Rank::King, Suit::Spades),
    ];
    let board = vec![
        card(Rank::Queen, Suit::Spades),
        card(Rank::Jack, Suit::Spades),
        card(Rank::Ten, Suit::Spades),
    ];
    let result = evaluate_hand(&hole, &board).unwrap();
    assert_eq!(classify_hand_strength(&result, &hole, &board, 0.99), "nuts");
}

#[test]
fn test_classify_full_house_very_strong() {
    let hole = vec![
        card(Rank::King, Suit::Hearts),
        card(Rank::King, Suit::Spades),
    ];
    let board = vec![
        card(Rank::King, Suit::Diamonds),
        card(Rank::Seven, Suit::Clubs),
        card(Rank::Seven, Suit::Hearts),
    ];
    let result = evaluate_hand(&hole, &board).unwrap();
    assert_eq!(
        classify_hand_strength(&result, &hole, &board, 0.95),
        "very_strong"
    );
}

#[test]
fn test_classify_straight_high_equity_very_strong() {
    let hole = vec![
        card(Rank::Nine, Suit::Hearts),
        card(Rank::Eight, Suit::Spades),
    ];
    let board = vec![
        card(Rank::Ten, Suit::Diamonds),
        card(Rank::Jack, Suit::Clubs),
        card(Rank::Seven, Suit::Hearts),
    ];
    let result = evaluate_hand(&hole, &board).unwrap();
    // With high equity, straight = very_strong
    assert_eq!(
        classify_hand_strength(&result, &hole, &board, 0.75),
        "very_strong"
    );
}

#[test]
fn test_classify_straight_low_equity_strong() {
    let hole = vec![
        card(Rank::Nine, Suit::Hearts),
        card(Rank::Eight, Suit::Spades),
    ];
    let board = vec![
        card(Rank::Ten, Suit::Diamonds),
        card(Rank::Jack, Suit::Clubs),
        card(Rank::Seven, Suit::Hearts),
    ];
    let result = evaluate_hand(&hole, &board).unwrap();
    assert_eq!(
        classify_hand_strength(&result, &hole, &board, 0.55),
        "strong"
    );
}

#[test]
fn test_classify_pocket_pair_set() {
    let hole = vec![
        card(Rank::Ten, Suit::Hearts),
        card(Rank::Ten, Suit::Diamonds),
    ];
    let board = vec![
        card(Rank::Ten, Suit::Clubs),
        card(Rank::Ace, Suit::Spades),
        card(Rank::Two, Suit::Hearts),
    ];
    let result = evaluate_hand(&hole, &board).unwrap();
    assert_eq!(
        classify_hand_strength(&result, &hole, &board, 0.90),
        "very_strong"
    );
}

#[test]
fn test_classify_top_pair_ace_kicker() {
    let hole = vec![
        card(Rank::Ace, Suit::Hearts),
        card(Rank::King, Suit::Spades),
    ];
    let board = vec![
        card(Rank::King, Suit::Diamonds),
        card(Rank::Seven, Suit::Clubs),
        card(Rank::Two, Suit::Hearts),
    ];
    let result = evaluate_hand(&hole, &board).unwrap();
    assert_eq!(
        classify_hand_strength(&result, &hole, &board, 0.72),
        "strong"
    );
}

#[test]
fn test_classify_top_pair_weak_kicker() {
    let hole = vec![
        card(Rank::King, Suit::Hearts),
        card(Rank::Four, Suit::Spades),
    ];
    let board = vec![
        card(Rank::King, Suit::Diamonds),
        card(Rank::Seven, Suit::Clubs),
        card(Rank::Two, Suit::Hearts),
    ];
    let result = evaluate_hand(&hole, &board).unwrap();
    assert_eq!(
        classify_hand_strength(&result, &hole, &board, 0.60),
        "medium"
    );
}

#[test]
fn test_classify_bottom_pair_weak() {
    let hole = vec![
        card(Rank::Two, Suit::Hearts),
        card(Rank::Three, Suit::Spades),
    ];
    let board = vec![
        card(Rank::Ace, Suit::Diamonds),
        card(Rank::King, Suit::Clubs),
        card(Rank::Two, Suit::Clubs),
    ];
    let result = evaluate_hand(&hole, &board).unwrap();
    assert_eq!(
        classify_hand_strength(&result, &hole, &board, 0.20),
        "weak"
    );
}

#[test]
fn test_classify_high_card_nothing() {
    let hole = vec![
        card(Rank::Nine, Suit::Hearts),
        card(Rank::Eight, Suit::Clubs),
    ];
    let board = vec![
        card(Rank::Ace, Suit::Spades),
        card(Rank::King, Suit::Diamonds),
        card(Rank::Two, Suit::Hearts),
    ];
    let result = evaluate_hand(&hole, &board).unwrap();
    assert_eq!(
        classify_hand_strength(&result, &hole, &board, 0.10),
        "weak"
    );
}

// ---------------------------------------------------------------------------
// Villain range estimator
// ---------------------------------------------------------------------------

#[test]
fn test_villain_range_not_empty() {
    let hero = vec![
        card(Rank::Ace, Suit::Hearts),
        card(Rank::King, Suit::Spades),
    ];
    for &sit in &["RFI", "vs_RFI", "vs_3bet", "bb_defense"] {
        let range = estimate_villain_range(sit, "BTN", Some("CO"), &hero, "6max");
        assert!(!range.is_empty(), "Range empty for situation: {}", sit);
    }
}

#[test]
fn test_villain_range_vs_3bet_is_tight() {
    let hero = vec![
        card(Rank::Two, Suit::Hearts),
        card(Rank::Three, Suit::Spades),
    ];
    let rfi_range = estimate_villain_range("RFI", "BTN", None, &hero, "6max");
    let three_bet_range = estimate_villain_range("vs_3bet", "BTN", Some("CO"), &hero, "6max");
    assert!(
        three_bet_range.len() < rfi_range.len(),
        "3-bet range should be tighter than RFI calling range"
    );
}

#[test]
fn test_villain_range_blockers() {
    let hero = vec![
        card(Rank::Ace, Suit::Hearts),
        card(Rank::Ace, Suit::Spades),
    ];
    let range = estimate_villain_range("RFI", "BTN", None, &hero, "6max");
    // AA should still be in range (hero blocks combos, not all combos)
    // but the count of combos is reduced
    assert!(!range.is_empty());
}

// ---------------------------------------------------------------------------
// Explanation helpers
// ---------------------------------------------------------------------------

#[test]
fn test_explain_hand_category_all() {
    let categories = [
        HandCategory::HighCard,
        HandCategory::OnePair,
        HandCategory::TwoPair,
        HandCategory::ThreeOfAKind,
        HandCategory::Straight,
        HandCategory::Flush,
        HandCategory::FullHouse,
        HandCategory::FourOfAKind,
        HandCategory::StraightFlush,
        HandCategory::RoyalFlush,
    ];
    for cat in &categories {
        let explanation = explain_hand_category(*cat);
        assert!(!explanation.is_empty(), "No explanation for {:?}", cat);
    }
}

#[test]
fn test_explain_board_texture_all() {
    for wetness in &[Wetness::Dry, Wetness::Medium, Wetness::Wet] {
        let explanation = explain_board_texture(*wetness);
        assert!(!explanation.is_empty());
    }
}

#[test]
fn test_explain_spr_all() {
    for zone in &[SprZone::Low, SprZone::Medium, SprZone::High] {
        let explanation = explain_spr(*zone);
        assert!(!explanation.is_empty());
    }
}

#[test]
fn test_explain_strength_all() {
    for s in &["nuts", "very_strong", "strong", "medium", "draw", "weak", "bluff"] {
        let explanation = explain_strength(s);
        assert!(!explanation.is_empty(), "No explanation for {}", s);
    }
}

// ---------------------------------------------------------------------------
// Interactive session (simulated I/O)
// ---------------------------------------------------------------------------

#[test]
fn test_session_quit_at_table_size() {
    let input = b"q\n";
    let mut reader: &[u8] = &input[..];
    let mut output = Vec::new();
    run_interactive_session(&mut reader, &mut output);
    let out = String::from_utf8(output).unwrap();
    assert!(out.contains("Welcome to GTO Play!"));
}

#[test]
fn test_session_preflop_raise_aks_btn() {
    // BTN, 2 players, AhKs, no prior raise -> RAISE, then quit
    let input = b"6max\n1/2\n200\nBTN\n2\nAhKs\nn\nn\n";
    let mut reader: &[u8] = &input[..];
    let mut output = Vec::new();
    run_interactive_session(&mut reader, &mut output);
    let out = String::from_utf8(output).unwrap();
    assert!(out.contains("RAISE"), "Expected RAISE for AKs on BTN, got:\n{}", out);
}

#[test]
fn test_session_preflop_fold_72o_utg() {
    // UTG, 2 players, 7h2c, no prior raise -> FOLD
    let input = b"6max\n1/2\n200\nUTG\n2\n7h2c\nn\nn\n";
    let mut reader: &[u8] = &input[..];
    let mut output = Vec::new();
    run_interactive_session(&mut reader, &mut output);
    let out = String::from_utf8(output).unwrap();
    assert!(out.contains("FOLD"), "Expected FOLD for 72o on UTG");
    assert!(out.contains("fold preflop"));
}

#[test]
fn test_session_invalid_cards_reprompt() {
    // Enter invalid cards first, then valid, then quit
    let input = b"6max\n1/2\n200\nBTN\n2\nZZ\nAhKs\nn\nn\n";
    let mut reader: &[u8] = &input[..];
    let mut output = Vec::new();
    run_interactive_session(&mut reader, &mut output);
    let out = String::from_utf8(output).unwrap();
    assert!(out.contains("Invalid cards"));
    assert!(out.contains("RAISE"));
}

#[test]
fn test_session_through_flop() {
    // BTN, AhKs, no raise, continue to flop Kd7c2h, bet, then don't continue to turn
    let input = b"6max\n1/2\n200\nBTN\n2\nAhKs\nn\ny\nKd7c2h\nbet\n5\nn\nn\n";
    let mut reader: &[u8] = &input[..];
    let mut output = Vec::new();
    run_interactive_session(&mut reader, &mut output);
    let out = String::from_utf8(output).unwrap();
    assert!(out.contains("Flop"));
    assert!(out.contains("Texture:"));
    assert!(out.contains("Equity vs villain:"));
}
