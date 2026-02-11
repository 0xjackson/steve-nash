use gto_cli::cards::*;
use gto_cli::postflop::*;

#[test]
fn test_dry_rainbow() {
    let board = parse_board("Ks7d2c").unwrap();
    let result = analyze_board(&board).unwrap();
    assert!(result.is_rainbow);
    assert_eq!(result.wetness, Wetness::Dry);
    assert_eq!(result.high_card, 'K');
    assert!(!result.is_paired);
}

#[test]
fn test_monotone() {
    let board = parse_board("Ts8s3s").unwrap();
    let result = analyze_board(&board).unwrap();
    assert!(result.is_monotone);
    assert_eq!(result.wetness, Wetness::Wet);
}

#[test]
fn test_paired() {
    let board = parse_board("KsKd7c").unwrap();
    let result = analyze_board(&board).unwrap();
    assert!(result.is_paired);
}

#[test]
fn test_connected() {
    let board = parse_board("9s8d7c").unwrap();
    let result = analyze_board(&board).unwrap();
    assert_eq!(result.connectedness, Connectedness::Connected);
    assert!(result.straight_draw_possible);
}

#[test]
fn test_two_tone() {
    let board = parse_board("AsKs7d").unwrap();
    let result = analyze_board(&board).unwrap();
    assert!(result.is_two_tone);
    assert!(result.flush_draw_possible);
}

#[test]
fn test_turn_board() {
    let board = parse_board("AsKdQhJs").unwrap();
    let result = analyze_board(&board).unwrap();
    assert_eq!(result.cards.len(), 4);
}

#[test]
fn test_too_few_cards() {
    let board = parse_board("AsKd").unwrap();
    assert!(analyze_board(&board).is_err());
}

#[test]
fn test_cbet_dry_ip() {
    let board = parse_board("Ks7d2c").unwrap();
    let texture = analyze_board(&board).unwrap();
    let rec = cbet_recommendation(&texture, "IP", 5.0, false);
    assert!(rec.should_cbet);
    assert!(rec.frequency >= 0.6);
    assert!(rec.sizing.contains("33%"));
}

#[test]
fn test_cbet_wet_oop() {
    let board = parse_board("Ts9s8d").unwrap();
    let texture = analyze_board(&board).unwrap();
    let rec = cbet_recommendation(&texture, "OOP", 5.0, false);
    assert!(rec.frequency < 0.5);
}

#[test]
fn test_cbet_multiway() {
    let board = parse_board("Ks7d2c").unwrap();
    let texture = analyze_board(&board).unwrap();
    let rec = cbet_recommendation(&texture, "IP", 5.0, true);
    assert!(rec.frequency < 0.5);
}

#[test]
fn test_bet_sizing_dry_flop() {
    let board = parse_board("Ks7d2c").unwrap();
    let texture = analyze_board(&board).unwrap();
    let size = bet_sizing(&texture, 8.0, "flop", false);
    assert!(size.contains("25") || size.contains("33"));
}

#[test]
fn test_bet_sizing_wet_flop() {
    let board = parse_board("Ts9s8d").unwrap();
    let texture = analyze_board(&board).unwrap();
    let size = bet_sizing(&texture, 8.0, "flop", false);
    assert!(size.contains("66") || size.contains("75"));
}

#[test]
fn test_bet_sizing_low_spr() {
    let board = parse_board("Ks7d2c").unwrap();
    let texture = analyze_board(&board).unwrap();
    let size = bet_sizing(&texture, 2.0, "flop", false);
    let lower = size.to_lowercase();
    assert!(lower.contains("low spr") || lower.contains("commit"));
}

#[test]
fn test_bet_sizing_polarized() {
    let board = parse_board("Ks7d2c").unwrap();
    let texture = analyze_board(&board).unwrap();
    let size = bet_sizing(&texture, 8.0, "river", true);
    assert!(size.contains("75") || size.contains("125"));
}

#[test]
fn test_street_strategy_nuts_bet() {
    let board = parse_board("Ks7d2c").unwrap();
    let texture = analyze_board(&board).unwrap();
    let result = street_strategy("nuts", &texture, 100.0, 500.0, "IP", "flop");
    assert_eq!(result.action, "BET");
}

#[test]
fn test_street_strategy_medium_check_oop() {
    let board = parse_board("Ks7d2c").unwrap();
    let texture = analyze_board(&board).unwrap();
    let result = street_strategy("medium", &texture, 100.0, 500.0, "OOP", "flop");
    assert!(result.action.contains("CHECK"));
}

#[test]
fn test_street_strategy_draw_semi_bluff() {
    let board = parse_board("Ts9s8d").unwrap();
    let texture = analyze_board(&board).unwrap();
    let result = street_strategy("draw", &texture, 100.0, 500.0, "IP", "flop");
    assert!(
        result.action.to_lowercase().contains("bluff") || result.action.contains("BET")
    );
}

#[test]
fn test_street_strategy_weak_fold() {
    let board = parse_board("Ks7d2c").unwrap();
    let texture = analyze_board(&board).unwrap();
    let result = street_strategy("weak", &texture, 100.0, 500.0, "OOP", "flop");
    assert!(result.action.contains("FOLD") || result.action.contains("CHECK"));
}
