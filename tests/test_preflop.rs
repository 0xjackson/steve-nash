use gto_cli::preflop::*;

#[test]
fn test_rfi_utg_6max() {
    let r = get_rfi_range("UTG", "6max");
    assert!(r.contains(&"AA".to_string()));
    assert!(r.contains(&"KK".to_string()));
    assert!(!r.iter().any(|h| h == "72o"));
}

#[test]
fn test_btn_has_more_than_utg() {
    let utg = get_rfi_range("UTG", "6max");
    let btn = get_rfi_range("BTN", "6max");
    assert!(btn.len() > utg.len());
}

#[test]
fn test_rfi_9max() {
    let r = get_rfi_range("UTG", "9max");
    assert!(r.contains(&"AA".to_string()));
}

#[test]
fn test_open_pct() {
    assert!(get_rfi_pct("UTG", "6max") > 0);
    assert!(get_rfi_pct("BTN", "6max") > get_rfi_pct("UTG", "6max"));
}

#[test]
fn test_vs_rfi_btn_vs_utg() {
    let result = get_vs_rfi_range("BTN", "UTG", "6max");
    assert!(result.three_bet.contains(&"AA".to_string()));
    assert!(!result.call.is_empty());
}

#[test]
fn test_vs_rfi_bb_vs_btn() {
    let result = get_vs_rfi_range("BB", "BTN", "6max");
    assert!(result.call.len() > 10); // BB defends wide vs BTN
    assert!(!result.three_bet.is_empty());
}

#[test]
fn test_vs_3bet_utg_vs_any() {
    let result = get_vs_3bet_range("UTG", "HJ", "6max");
    assert!(result.four_bet.contains(&"AA".to_string()));
    assert!(result.call.contains(&"QQ".to_string()));
}

#[test]
fn test_vs_3bet_btn_vs_sb() {
    let result = get_vs_3bet_range("BTN", "SB", "6max");
    assert!(result.call.len() > 5);
}

#[test]
fn test_squeeze_sb() {
    let r = get_squeeze_range("SB", "UTG", "HJ", "6max");
    assert!(r.contains(&"AA".to_string()));
}

#[test]
fn test_bb_defense_vs_btn() {
    let r = get_bb_defense("BTN", "6max");
    assert!(r.call.len() > 20);
    assert!(r.three_bet.len() > 5);
}

#[test]
fn test_preflop_action_rfi_raise() {
    let result = preflop_action("AA", "UTG", "RFI", None, "6max").unwrap();
    assert_eq!(result.action, "RAISE");
}

#[test]
fn test_preflop_action_rfi_fold() {
    let result = preflop_action("72o", "UTG", "RFI", None, "6max").unwrap();
    assert_eq!(result.action, "FOLD");
}

#[test]
fn test_preflop_action_vs_rfi_3bet() {
    let result =
        preflop_action("AA", "BTN", "vs_RFI", Some("UTG"), "6max").unwrap();
    assert_eq!(result.action, "3BET");
}

#[test]
fn test_preflop_action_vs_rfi_call() {
    let result =
        preflop_action("AQs", "BTN", "vs_RFI", Some("UTG"), "6max").unwrap();
    assert_eq!(result.action, "CALL");
}

#[test]
fn test_preflop_action_vs_3bet_4bet() {
    let result =
        preflop_action("AA", "UTG", "vs_3bet", Some("HJ"), "6max").unwrap();
    assert_eq!(result.action, "4BET");
}

#[test]
fn test_preflop_action_missing_villain() {
    let result = preflop_action("AA", "BTN", "vs_RFI", None, "6max");
    assert!(result.is_err());
}

#[test]
fn test_positions_6max() {
    assert_eq!(positions_for("6max").len(), 6);
}

#[test]
fn test_positions_9max() {
    assert_eq!(positions_for("9max").len(), 9);
}
