use gto_cli::cards::*;
use gto_cli::hand_evaluator::*;

fn c(notation: &str) -> Card {
    parse_card(notation).unwrap()
}

#[test]
fn test_royal_flush() {
    let hole = vec![c("As"), c("Ks")];
    let board = parse_board("QsTsJs2h3d").unwrap();
    let result = evaluate_hand(&hole, &board).unwrap();
    assert_eq!(result.category, HandCategory::RoyalFlush);
    assert_eq!(result.rank, 9);
}

#[test]
fn test_straight_flush() {
    let hole = vec![c("9h"), c("8h")];
    let board = parse_board("7h6h5hAcKd").unwrap();
    let result = evaluate_hand(&hole, &board).unwrap();
    assert_eq!(result.category, HandCategory::StraightFlush);
}

#[test]
fn test_four_of_a_kind() {
    let hole = vec![c("Ks"), c("Kh")];
    let board = parse_board("KdKc5s2h3d").unwrap();
    let result = evaluate_hand(&hole, &board).unwrap();
    assert_eq!(result.category, HandCategory::FourOfAKind);
}

#[test]
fn test_full_house() {
    let hole = vec![c("As"), c("Ah")];
    let board = parse_board("AdKsKh2c3d").unwrap();
    let result = evaluate_hand(&hole, &board).unwrap();
    assert_eq!(result.category, HandCategory::FullHouse);
    assert_eq!(result.kickers, vec![14, 13]);
}

#[test]
fn test_flush() {
    let hole = vec![c("As"), c("Ts")];
    let board = parse_board("8s5s2sKdQh").unwrap();
    let result = evaluate_hand(&hole, &board).unwrap();
    assert_eq!(result.category, HandCategory::Flush);
}

#[test]
fn test_straight() {
    let hole = vec![c("9s"), c("8h")];
    let board = parse_board("7d6c5sAhKd").unwrap();
    let result = evaluate_hand(&hole, &board).unwrap();
    assert_eq!(result.category, HandCategory::Straight);
    assert_eq!(result.kickers, vec![9]);
}

#[test]
fn test_wheel() {
    let hole = vec![c("As"), c("2h")];
    let board = parse_board("3d4c5sKhQd").unwrap();
    let result = evaluate_hand(&hole, &board).unwrap();
    assert_eq!(result.category, HandCategory::Straight);
    assert_eq!(result.kickers, vec![5]);
}

#[test]
fn test_three_of_a_kind() {
    let hole = vec![c("Qs"), c("Qh")];
    let board = parse_board("Qd7s3h2cKd").unwrap();
    let result = evaluate_hand(&hole, &board).unwrap();
    assert_eq!(result.category, HandCategory::ThreeOfAKind);
}

#[test]
fn test_two_pair() {
    let hole = vec![c("As"), c("Kh")];
    let board = parse_board("AdKs5c2h3d").unwrap();
    let result = evaluate_hand(&hole, &board).unwrap();
    assert_eq!(result.category, HandCategory::TwoPair);
    assert_eq!(result.kickers, vec![14, 13, 5]);
}

#[test]
fn test_one_pair() {
    let hole = vec![c("As"), c("Ah")];
    let board = parse_board("Kd7s3c2h5d").unwrap();
    let result = evaluate_hand(&hole, &board).unwrap();
    assert_eq!(result.category, HandCategory::OnePair);
    assert_eq!(result.kickers, vec![14, 13, 7, 5]);
}

#[test]
fn test_high_card() {
    let hole = vec![c("As"), c("Kh")];
    let board = parse_board("Qd9s3c2h5d").unwrap();
    let result = evaluate_hand(&hole, &board).unwrap();
    assert_eq!(result.category, HandCategory::HighCard);
}

#[test]
fn test_not_enough_cards() {
    assert!(evaluate_hand(&[c("As"), c("Kh")], &[c("Qd")]).is_err());
}

#[test]
fn test_flush_beats_straight() {
    let board = parse_board("7s6s5s4dAh").unwrap();
    assert_eq!(
        compare_hands(&[c("As"), c("2s")], &[c("8h"), c("9h")], &board).unwrap(),
        1
    );
}

#[test]
fn test_higher_pair_wins() {
    let board = parse_board("2s5d8cTh3d").unwrap();
    assert_eq!(
        compare_hands(&[c("As"), c("Ah")], &[c("Ks"), c("Kh")], &board).unwrap(),
        1
    );
}

#[test]
fn test_kicker_decides() {
    let board = parse_board("As5d8cTh3d").unwrap();
    assert_eq!(
        compare_hands(&[c("Ad"), c("Kh")], &[c("Ah"), c("Qd")], &board).unwrap(),
        1
    );
}

#[test]
fn test_tie() {
    let board = parse_board("AsKdQhJsTs").unwrap();
    assert_eq!(
        compare_hands(&[c("2h"), c("3d")], &[c("4h"), c("5d")], &board).unwrap(),
        0
    );
}

#[test]
fn test_two_pair_kicker() {
    let board = parse_board("AsAd5s5d2c").unwrap();
    let r = compare_hands(&[c("Kh"), c("3c")], &[c("Qh"), c("3d")], &board).unwrap();
    assert_eq!(r, 1);
}

#[test]
fn test_hand_result_ordering() {
    let high = HandResult::new(0, HandCategory::HighCard, vec![14, 13, 12, 11, 9], vec![]);
    let pair = HandResult::new(1, HandCategory::OnePair, vec![14, 13, 12, 11], vec![]);
    assert!(pair > high);
    assert!(high < pair);
}

#[test]
fn test_hand_result_kicker() {
    let h1 = HandResult::new(1, HandCategory::OnePair, vec![14, 13, 12, 11], vec![]);
    let h2 = HandResult::new(1, HandCategory::OnePair, vec![14, 13, 12, 10], vec![]);
    assert!(h1 > h2);
}
