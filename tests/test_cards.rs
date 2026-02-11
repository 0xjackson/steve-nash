use gto_cli::cards::*;
use gto_cli::error::GtoError;

#[test]
fn test_card_creation() {
    let c = Card::new(Rank::Ace, Suit::Spades);
    assert_eq!(c.rank, Rank::Ace);
    assert_eq!(c.suit, Suit::Spades);
    assert_eq!(c.value(), 14);
}

#[test]
fn test_invalid_rank() {
    assert!(Rank::from_char('X').is_err());
}

#[test]
fn test_invalid_suit() {
    assert!(Suit::from_char('x').is_err());
}

#[test]
fn test_card_str() {
    let c = Card::new(Rank::King, Suit::Diamonds);
    assert_eq!(format!("{}", c), "Kd");
}

#[test]
fn test_card_pretty() {
    let c = Card::new(Rank::Ace, Suit::Spades);
    assert_eq!(c.pretty(), "A\u{2660}");
}

#[test]
fn test_card_ordering() {
    let two = Card::new(Rank::Two, Suit::Spades);
    let ace = Card::new(Rank::Ace, Suit::Spades);
    assert!(two < ace);
    let king = Card::new(Rank::King, Suit::Hearts);
    let queen = Card::new(Rank::Queen, Suit::Diamonds);
    assert!(!(king < queen));
}

#[test]
fn test_card_equality() {
    let a1 = Card::new(Rank::Ace, Suit::Spades);
    let a2 = Card::new(Rank::Ace, Suit::Spades);
    let a3 = Card::new(Rank::Ace, Suit::Hearts);
    assert_eq!(a1, a2);
    assert_ne!(a1, a3);
}

#[test]
fn test_card_hashable() {
    use std::collections::HashSet;
    let mut s = HashSet::new();
    s.insert(Card::new(Rank::Ace, Suit::Spades));
    s.insert(Card::new(Rank::Ace, Suit::Spades)); // duplicate
    s.insert(Card::new(Rank::King, Suit::Hearts));
    assert_eq!(s.len(), 2);
}

#[test]
fn test_parse_card_basic() {
    assert_eq!(parse_card("As").unwrap(), Card::new(Rank::Ace, Suit::Spades));
    assert_eq!(parse_card("Td").unwrap(), Card::new(Rank::Ten, Suit::Diamonds));
}

#[test]
fn test_parse_card_case_insensitive_suit() {
    assert_eq!(parse_card("AH").unwrap(), Card::new(Rank::Ace, Suit::Hearts));
}

#[test]
fn test_parse_card_invalid() {
    assert!(parse_card("ABC").is_err());
}

#[test]
fn test_parse_board_flop() {
    let board = parse_board("AsKdQh").unwrap();
    assert_eq!(board.len(), 3);
    assert_eq!(board[0], Card::new(Rank::Ace, Suit::Spades));
}

#[test]
fn test_parse_board_with_spaces() {
    let board = parse_board("As Kd Qh").unwrap();
    assert_eq!(board.len(), 3);
}

#[test]
fn test_parse_board_turn() {
    let board = parse_board("AsKdQh5c").unwrap();
    assert_eq!(board.len(), 4);
}

#[test]
fn test_parse_board_river() {
    let board = parse_board("As Kd Qh 5c 2s").unwrap();
    assert_eq!(board.len(), 5);
}

#[test]
fn test_deck_full() {
    let d = Deck::new(None);
    assert_eq!(d.len(), 52);
}

#[test]
fn test_deck_exclude() {
    let excluded = vec![
        Card::new(Rank::Ace, Suit::Spades),
        Card::new(Rank::King, Suit::Hearts),
    ];
    let d = Deck::new(Some(&excluded));
    assert_eq!(d.len(), 50);
}

#[test]
fn test_deck_deal() {
    let mut d = Deck::new(None);
    let cards = d.deal(5).unwrap();
    assert_eq!(cards.len(), 5);
    assert_eq!(d.len(), 47);
}

#[test]
fn test_deck_deal_too_many() {
    let mut d = Deck::new(None);
    assert!(d.deal(53).is_err());
}

#[test]
fn test_deck_shuffle() {
    let mut d = Deck::new(None);
    let original: std::collections::HashSet<Card> = d.cards.iter().copied().collect();
    d.shuffle();
    assert_eq!(d.len(), 52);
    let shuffled: std::collections::HashSet<Card> = d.cards.iter().copied().collect();
    assert_eq!(original, shuffled);
}

#[test]
fn test_simplify_pair() {
    let cards = vec![
        Card::new(Rank::Ace, Suit::Spades),
        Card::new(Rank::Ace, Suit::Hearts),
    ];
    assert_eq!(simplify_hand(&cards).unwrap(), "AA");
}

#[test]
fn test_simplify_suited() {
    let cards = vec![
        Card::new(Rank::Ace, Suit::Spades),
        Card::new(Rank::King, Suit::Spades),
    ];
    assert_eq!(simplify_hand(&cards).unwrap(), "AKs");
}

#[test]
fn test_simplify_offsuit() {
    let cards = vec![
        Card::new(Rank::Ace, Suit::Spades),
        Card::new(Rank::King, Suit::Hearts),
    ];
    assert_eq!(simplify_hand(&cards).unwrap(), "AKo");
}

#[test]
fn test_simplify_ordering() {
    let cards1 = vec![
        Card::new(Rank::King, Suit::Spades),
        Card::new(Rank::Ace, Suit::Spades),
    ];
    assert_eq!(simplify_hand(&cards1).unwrap(), "AKs");

    let cards2 = vec![
        Card::new(Rank::Nine, Suit::Hearts),
        Card::new(Rank::Ten, Suit::Diamonds),
    ];
    assert_eq!(simplify_hand(&cards2).unwrap(), "T9o");
}

#[test]
fn test_hand_combos_pair() {
    let combos = hand_combos("AA").unwrap();
    assert_eq!(combos.len(), 6);
}

#[test]
fn test_hand_combos_suited() {
    let combos = hand_combos("AKs").unwrap();
    assert_eq!(combos.len(), 4);
    for (c1, c2) in &combos {
        assert_eq!(c1.suit, c2.suit);
    }
}

#[test]
fn test_hand_combos_offsuit() {
    let combos = hand_combos("AKo").unwrap();
    assert_eq!(combos.len(), 12);
    for (c1, c2) in &combos {
        assert_ne!(c1.suit, c2.suit);
    }
}

#[test]
fn test_hand_combos_specific() {
    let combos = hand_combos("AsKh").unwrap();
    assert_eq!(combos.len(), 1);
    assert_eq!(combos[0].0, Card::new(Rank::Ace, Suit::Spades));
    assert_eq!(combos[0].1, Card::new(Rank::King, Suit::Hearts));
}
