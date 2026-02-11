use gto_cli::cards::*;
use gto_cli::equity::*;

fn c(notation: &str) -> Card {
    parse_card(notation).unwrap()
}

#[test]
fn test_aa_vs_kk() {
    let result = equity_vs_hand(
        &[c("As"), c("Ah")],
        &[c("Ks"), c("Kh")],
        None,
        10000,
    )
    .unwrap();
    assert!(result.equity() > 0.75);
    assert!(result.equity() < 0.88);
}

#[test]
fn test_aa_vs_kk_on_flop() {
    let board = parse_board("2s5d8c").unwrap();
    let result = equity_vs_hand(
        &[c("As"), c("Ah")],
        &[c("Ks"), c("Kh")],
        Some(&board),
        10000,
    )
    .unwrap();
    assert!(result.equity() > 0.85);
}

#[test]
fn test_coinflip() {
    let result = equity_vs_hand(
        &[c("As"), c("Ks")],
        &[c("Qh"), c("Qd")],
        None,
        10000,
    )
    .unwrap();
    assert!(result.equity() > 0.40);
    assert!(result.equity() < 0.60);
}

#[test]
fn test_made_hand_vs_draw() {
    let board = parse_board("Ts9s2h").unwrap();
    let result = equity_vs_hand(
        &[c("Td"), c("Th")], // set of tens
        &[c("As"), c("Ks")], // nut flush draw
        Some(&board),
        10000,
    )
    .unwrap();
    assert!(result.equity() > 0.50);
}

#[test]
fn test_result_string() {
    let result = equity_vs_hand(
        &[c("As"), c("Ah")],
        &[c("Ks"), c("Kh")],
        None,
        1000,
    )
    .unwrap();
    let s = format!("{}", result);
    assert!(s.contains("Win"));
    assert!(s.contains("equity"));
}

#[test]
fn test_aa_vs_premium_range() {
    let result = equity_vs_range(
        &[c("As"), c("Ah")],
        &["KK".to_string(), "QQ".to_string(), "JJ".to_string()],
        None,
        5000,
    )
    .unwrap();
    assert!(result.equity() > 0.70);
}

#[test]
fn test_no_valid_combos() {
    let result = equity_vs_range(
        &[c("As"), c("Ah")],
        &["AsAh".to_string()], // exact combo blocked
        None,
        100,
    );
    assert!(result.is_err());
}
