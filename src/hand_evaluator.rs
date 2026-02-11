use std::cmp::Ordering;
use std::fmt;

use itertools::Itertools;

use crate::cards::Card;
use crate::error::{GtoError, GtoResult};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum HandCategory {
    HighCard = 0,
    OnePair = 1,
    TwoPair = 2,
    ThreeOfAKind = 3,
    Straight = 4,
    Flush = 5,
    FullHouse = 6,
    FourOfAKind = 7,
    StraightFlush = 8,
    RoyalFlush = 9,
}

impl fmt::Display for HandCategory {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            HandCategory::HighCard => write!(f, "High Card"),
            HandCategory::OnePair => write!(f, "One Pair"),
            HandCategory::TwoPair => write!(f, "Two Pair"),
            HandCategory::ThreeOfAKind => write!(f, "Three of a Kind"),
            HandCategory::Straight => write!(f, "Straight"),
            HandCategory::Flush => write!(f, "Flush"),
            HandCategory::FullHouse => write!(f, "Full House"),
            HandCategory::FourOfAKind => write!(f, "Four of a Kind"),
            HandCategory::StraightFlush => write!(f, "Straight Flush"),
            HandCategory::RoyalFlush => write!(f, "Royal Flush"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct HandResult {
    pub rank: u8,
    pub category: HandCategory,
    pub kickers: Vec<u8>,
    pub cards: Vec<Card>,
}

impl HandResult {
    pub fn new(rank: u8, category: HandCategory, kickers: Vec<u8>, cards: Vec<Card>) -> Self {
        HandResult {
            rank,
            category,
            kickers,
            cards,
        }
    }
}

impl fmt::Display for HandResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.category)
    }
}

impl PartialEq for HandResult {
    fn eq(&self, other: &Self) -> bool {
        self.rank == other.rank && self.kickers == other.kickers
    }
}

impl Eq for HandResult {}

impl PartialOrd for HandResult {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for HandResult {
    fn cmp(&self, other: &Self) -> Ordering {
        match self.rank.cmp(&other.rank) {
            Ordering::Equal => self.kickers.cmp(&other.kickers),
            ord => ord,
        }
    }
}

fn is_flush(cards: &[Card]) -> bool {
    cards.windows(2).all(|w| w[0].suit == w[1].suit)
}

fn is_straight(values: &[u8]) -> Option<u8> {
    let mut unique: Vec<u8> = values.iter().copied().collect::<std::collections::BTreeSet<u8>>().into_iter().collect();
    unique.sort_unstable();
    unique.reverse();

    if unique.len() < 5 {
        return None;
    }

    if unique.len() == 5 && unique[0] - unique[4] == 4 {
        return Some(unique[0]);
    }

    // Wheel: A-2-3-4-5
    let set: std::collections::HashSet<u8> = values.iter().copied().collect();
    if set.contains(&14) && set.contains(&2) && set.contains(&3) && set.contains(&4) && set.contains(&5) {
        return Some(5);
    }

    None
}

fn evaluate_five(cards: &[Card; 5]) -> HandResult {
    let mut values: Vec<u8> = cards.iter().map(|c| c.value()).collect();
    values.sort_unstable_by(|a, b| b.cmp(a));

    let flush = is_flush(cards);
    let straight_high = is_straight(&values);

    // Count values
    let mut counts = [0u8; 15];
    for &v in &values {
        counts[v as usize] += 1;
    }

    if flush && straight_high.is_some() {
        let high = straight_high.unwrap();
        if high == 14 {
            return HandResult::new(9, HandCategory::RoyalFlush, vec![14], cards.to_vec());
        }
        return HandResult::new(8, HandCategory::StraightFlush, vec![high], cards.to_vec());
    }

    // Build frequency list: (count, value) sorted by count desc, then value desc
    let mut freq: Vec<(u8, u8)> = Vec::new();
    for v in (2..=14u8).rev() {
        if counts[v as usize] > 0 {
            freq.push((counts[v as usize], v));
        }
    }
    freq.sort_by(|a, b| b.0.cmp(&a.0).then(b.1.cmp(&a.1)));

    // Four of a kind
    if freq[0].0 == 4 {
        let quad_val = freq[0].1;
        let kicker = values.iter().find(|&&v| v != quad_val).copied().unwrap();
        return HandResult::new(7, HandCategory::FourOfAKind, vec![quad_val, kicker], cards.to_vec());
    }

    // Full house
    if freq[0].0 == 3 && freq[1].0 == 2 {
        return HandResult::new(6, HandCategory::FullHouse, vec![freq[0].1, freq[1].1], cards.to_vec());
    }

    // Flush
    if flush {
        return HandResult::new(5, HandCategory::Flush, values.clone(), cards.to_vec());
    }

    // Straight
    if let Some(high) = straight_high {
        return HandResult::new(4, HandCategory::Straight, vec![high], cards.to_vec());
    }

    // Three of a kind
    if freq[0].0 == 3 {
        let trip_val = freq[0].1;
        let mut kicks: Vec<u8> = values.iter().filter(|&&v| v != trip_val).copied().collect();
        kicks.sort_unstable_by(|a, b| b.cmp(a));
        let mut kickers = vec![trip_val];
        kickers.extend(kicks);
        return HandResult::new(3, HandCategory::ThreeOfAKind, kickers, cards.to_vec());
    }

    // Two pair
    let mut pair_vals: Vec<u8> = (2..=14)
        .filter(|&v| counts[v as usize] == 2)
        .collect();
    pair_vals.sort_unstable_by(|a, b| b.cmp(a));

    if pair_vals.len() == 2 {
        let kicker = values
            .iter()
            .find(|&&v| !pair_vals.contains(&v))
            .copied()
            .unwrap();
        return HandResult::new(
            2,
            HandCategory::TwoPair,
            vec![pair_vals[0], pair_vals[1], kicker],
            cards.to_vec(),
        );
    }

    // One pair
    if pair_vals.len() == 1 {
        let pair_val = pair_vals[0];
        let mut kicks: Vec<u8> = values.iter().filter(|&&v| v != pair_val).copied().collect();
        kicks.sort_unstable_by(|a, b| b.cmp(a));
        let mut kickers = vec![pair_val];
        kickers.extend(kicks);
        return HandResult::new(1, HandCategory::OnePair, kickers, cards.to_vec());
    }

    // High card
    HandResult::new(0, HandCategory::HighCard, values, cards.to_vec())
}

pub fn evaluate_hand(hole_cards: &[Card], board: &[Card]) -> GtoResult<HandResult> {
    let mut all_cards: Vec<Card> = Vec::with_capacity(hole_cards.len() + board.len());
    all_cards.extend_from_slice(hole_cards);
    all_cards.extend_from_slice(board);

    if all_cards.len() < 5 {
        return Err(GtoError::NotEnoughCards {
            need: 5,
            got: all_cards.len(),
        });
    }

    let mut best: Option<HandResult> = None;
    for combo in all_cards.iter().combinations(5) {
        let five: [Card; 5] = [*combo[0], *combo[1], *combo[2], *combo[3], *combo[4]];
        let result = evaluate_five(&five);
        if best.as_ref().map_or(true, |b| result > *b) {
            best = Some(result);
        }
    }

    Ok(best.unwrap())
}

pub fn compare_hands(hand1: &[Card], hand2: &[Card], board: &[Card]) -> GtoResult<i32> {
    let r1 = evaluate_hand(hand1, board)?;
    let r2 = evaluate_hand(hand2, board)?;
    Ok(match r1.cmp(&r2) {
        Ordering::Greater => 1,
        Ordering::Less => -1,
        Ordering::Equal => 0,
    })
}
