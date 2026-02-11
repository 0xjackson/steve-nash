use std::fmt;
use std::hash::{Hash, Hasher};

use rand::seq::SliceRandom;
use rand::thread_rng;

use crate::error::{GtoError, GtoResult};

pub const RANKS_STR: &str = "23456789TJQKA";
pub const SUITS_STR: &str = "shdc";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Rank {
    Two = 2,
    Three = 3,
    Four = 4,
    Five = 5,
    Six = 6,
    Seven = 7,
    Eight = 8,
    Nine = 9,
    Ten = 10,
    Jack = 11,
    Queen = 12,
    King = 13,
    Ace = 14,
}

impl Rank {
    pub fn from_char(c: char) -> GtoResult<Rank> {
        match c {
            '2' => Ok(Rank::Two),
            '3' => Ok(Rank::Three),
            '4' => Ok(Rank::Four),
            '5' => Ok(Rank::Five),
            '6' => Ok(Rank::Six),
            '7' => Ok(Rank::Seven),
            '8' => Ok(Rank::Eight),
            '9' => Ok(Rank::Nine),
            'T' => Ok(Rank::Ten),
            'J' => Ok(Rank::Jack),
            'Q' => Ok(Rank::Queen),
            'K' => Ok(Rank::King),
            'A' => Ok(Rank::Ace),
            _ => Err(GtoError::InvalidRank(c)),
        }
    }

    pub fn to_char(self) -> char {
        match self {
            Rank::Two => '2',
            Rank::Three => '3',
            Rank::Four => '4',
            Rank::Five => '5',
            Rank::Six => '6',
            Rank::Seven => '7',
            Rank::Eight => '8',
            Rank::Nine => '9',
            Rank::Ten => 'T',
            Rank::Jack => 'J',
            Rank::Queen => 'Q',
            Rank::King => 'K',
            Rank::Ace => 'A',
        }
    }

    pub fn value(self) -> u8 {
        self as u8
    }
}

pub const ALL_RANKS: [Rank; 13] = [
    Rank::Two,
    Rank::Three,
    Rank::Four,
    Rank::Five,
    Rank::Six,
    Rank::Seven,
    Rank::Eight,
    Rank::Nine,
    Rank::Ten,
    Rank::Jack,
    Rank::Queen,
    Rank::King,
    Rank::Ace,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Suit {
    Spades,
    Hearts,
    Diamonds,
    Clubs,
}

impl Suit {
    pub fn from_char(c: char) -> GtoResult<Suit> {
        match c.to_ascii_lowercase() {
            's' => Ok(Suit::Spades),
            'h' => Ok(Suit::Hearts),
            'd' => Ok(Suit::Diamonds),
            'c' => Ok(Suit::Clubs),
            _ => Err(GtoError::InvalidSuit(c)),
        }
    }

    pub fn to_char(self) -> char {
        match self {
            Suit::Spades => 's',
            Suit::Hearts => 'h',
            Suit::Diamonds => 'd',
            Suit::Clubs => 'c',
        }
    }

    pub fn symbol(self) -> &'static str {
        match self {
            Suit::Spades => "\u{2660}",
            Suit::Hearts => "\u{2665}",
            Suit::Diamonds => "\u{2666}",
            Suit::Clubs => "\u{2663}",
        }
    }
}

pub const ALL_SUITS: [Suit; 4] = [Suit::Spades, Suit::Hearts, Suit::Diamonds, Suit::Clubs];

#[derive(Debug, Clone, Copy, Eq)]
pub struct Card {
    pub rank: Rank,
    pub suit: Suit,
}

impl Card {
    pub fn new(rank: Rank, suit: Suit) -> Card {
        Card { rank, suit }
    }

    pub fn value(&self) -> u8 {
        self.rank.value()
    }

    pub fn pretty(&self) -> String {
        format!("{}{}", self.rank.to_char(), self.suit.symbol())
    }
}

impl fmt::Display for Card {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}{}", self.rank.to_char(), self.suit.to_char())
    }
}

impl PartialEq for Card {
    fn eq(&self, other: &Self) -> bool {
        self.rank == other.rank && self.suit == other.suit
    }
}

impl Hash for Card {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.rank.hash(state);
        self.suit.hash(state);
    }
}

impl PartialOrd for Card {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Card {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.rank.cmp(&other.rank)
    }
}

pub struct Deck {
    pub cards: Vec<Card>,
}

impl Deck {
    pub fn new(exclude: Option<&[Card]>) -> Deck {
        let excluded: std::collections::HashSet<Card> = exclude
            .map(|e| e.iter().copied().collect())
            .unwrap_or_default();
        let cards = ALL_RANKS
            .iter()
            .flat_map(|&r| ALL_SUITS.iter().map(move |&s| Card::new(r, s)))
            .filter(|c| !excluded.contains(c))
            .collect();
        Deck { cards }
    }

    pub fn shuffle(&mut self) -> &mut Self {
        let mut rng = thread_rng();
        self.cards.shuffle(&mut rng);
        self
    }

    pub fn deal(&mut self, n: usize) -> GtoResult<Vec<Card>> {
        if n > self.cards.len() {
            return Err(GtoError::NotEnoughDeck {
                requested: n,
                available: self.cards.len(),
            });
        }
        let dealt: Vec<Card> = self.cards.drain(..n).collect();
        Ok(dealt)
    }

    pub fn len(&self) -> usize {
        self.cards.len()
    }
}

pub fn parse_card(notation: &str) -> GtoResult<Card> {
    let notation = notation.trim();
    let chars: Vec<char> = notation.chars().collect();
    if chars.len() != 2 {
        return Err(GtoError::InvalidCardNotation(notation.to_string()));
    }
    let rank = Rank::from_char(chars[0].to_ascii_uppercase())?;
    let suit = Suit::from_char(chars[1])?;
    Ok(Card::new(rank, suit))
}

pub fn parse_board(notation: &str) -> GtoResult<Vec<Card>> {
    let notation = notation.trim().replace(' ', "").replace(',', "");
    if notation.len() % 2 != 0 {
        return Err(GtoError::InvalidBoardNotation(notation.to_string()));
    }
    let mut cards = Vec::new();
    let chars: Vec<char> = notation.chars().collect();
    for i in (0..chars.len()).step_by(2) {
        let s: String = chars[i..i + 2].iter().collect();
        cards.push(parse_card(&s)?);
    }
    Ok(cards)
}

pub fn simplify_hand(cards: &[Card]) -> GtoResult<String> {
    if cards.len() != 2 {
        return Err(GtoError::InvalidHandSize);
    }
    let (c1, c2) = (cards[0], cards[1]);
    let (r1, r2) = if c1.rank >= c2.rank {
        (c1.rank, c2.rank)
    } else {
        (c2.rank, c1.rank)
    };

    if r1 == r2 {
        return Ok(format!("{}{}", r1.to_char(), r2.to_char()));
    }

    let suffix = if c1.suit == c2.suit { "s" } else { "o" };
    Ok(format!("{}{}{}", r1.to_char(), r2.to_char(), suffix))
}

pub fn hand_combos(notation: &str) -> GtoResult<Vec<(Card, Card)>> {
    let notation = notation.trim();
    let chars: Vec<char> = notation.chars().collect();

    // Pair notation: "AA"
    if chars.len() == 2 && chars[0] == chars[1] {
        let rank = Rank::from_char(chars[0])?;
        let mut combos = Vec::new();
        for i in 0..ALL_SUITS.len() {
            for j in (i + 1)..ALL_SUITS.len() {
                combos.push((Card::new(rank, ALL_SUITS[i]), Card::new(rank, ALL_SUITS[j])));
            }
        }
        return Ok(combos);
    }

    // Suited/offsuit notation: "AKs" or "AKo"
    if chars.len() == 3 {
        let r1 = Rank::from_char(chars[0])?;
        let r2 = Rank::from_char(chars[1])?;
        let kind = chars[2];

        if kind == 's' {
            let combos = ALL_SUITS
                .iter()
                .map(|&s| (Card::new(r1, s), Card::new(r2, s)))
                .collect();
            return Ok(combos);
        } else if kind == 'o' {
            let mut combos = Vec::new();
            for &s1 in &ALL_SUITS {
                for &s2 in &ALL_SUITS {
                    if s1 != s2 {
                        combos.push((Card::new(r1, s1), Card::new(r2, s2)));
                    }
                }
            }
            return Ok(combos);
        }
    }

    // Specific cards: "AsKh"
    if chars.len() == 4 {
        let c1 = parse_card(&notation[..2])?;
        let c2 = parse_card(&notation[2..])?;
        return Ok(vec![(c1, c2)]);
    }

    Err(GtoError::InvalidHandNotation(notation.to_string()))
}

/// Returns the index of a rank char in RANKS_STR (0-based: '2'=0, 'A'=12)
pub fn rank_index(c: char) -> Option<usize> {
    RANKS_STR.find(c)
}
