use std::collections::{HashMap, HashSet};

use crate::cards::Card;
use crate::error::{GtoError, GtoResult};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Connectedness {
    Disconnected,
    SemiConnected,
    Connected,
}

impl std::fmt::Display for Connectedness {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Connectedness::Disconnected => write!(f, "disconnected"),
            Connectedness::SemiConnected => write!(f, "semi-connected"),
            Connectedness::Connected => write!(f, "connected"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Wetness {
    Dry,
    Medium,
    Wet,
}

impl std::fmt::Display for Wetness {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Wetness::Dry => write!(f, "dry"),
            Wetness::Medium => write!(f, "medium"),
            Wetness::Wet => write!(f, "wet"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct BoardTexture {
    pub cards: Vec<Card>,
    pub high_card: char,
    pub is_paired: bool,
    pub is_monotone: bool,
    pub is_two_tone: bool,
    pub is_rainbow: bool,
    pub flush_draw_possible: bool,
    pub straight_draw_possible: bool,
    pub connectedness: Connectedness,
    pub wetness: Wetness,
    pub category: String,
    pub draws: Vec<String>,
}

pub fn analyze_board(board_cards: &[Card]) -> GtoResult<BoardTexture> {
    if board_cards.len() < 3 {
        return Err(GtoError::NotEnoughCards {
            need: 3,
            got: board_cards.len(),
        });
    }

    let mut values: Vec<u8> = board_cards.iter().map(|c| c.value()).collect();
    values.sort_unstable_by(|a, b| b.cmp(a));

    let suits: Vec<_> = board_cards.iter().map(|c| c.suit).collect();
    let mut suit_counts: HashMap<_, u32> = HashMap::new();
    for &s in &suits {
        *suit_counts.entry(s).or_insert(0) += 1;
    }
    let max_suit = *suit_counts.values().max().unwrap();

    // Check first 3 cards for monotone
    let first_three_same = board_cards.len() >= 3 && {
        let s: HashSet<_> = suits[..3].iter().collect();
        s.len() == 1
    };
    let is_monotone = max_suit >= 3 && first_three_same;
    let is_two_tone = !is_monotone && max_suit >= 2;
    let is_rainbow = max_suit == 1;

    let mut rank_counts: HashMap<u8, u32> = HashMap::new();
    for &v in &values {
        *rank_counts.entry(v).or_insert(0) += 1;
    }
    let is_paired = *rank_counts.values().max().unwrap() >= 2;

    let mut unique_vals: Vec<u8> = values.iter().copied().collect::<HashSet<_>>().into_iter().collect();
    unique_vals.sort_unstable();

    let mut gaps: Vec<u8> = Vec::new();
    for i in 0..unique_vals.len().saturating_sub(1) {
        gaps.push(unique_vals[i + 1] - unique_vals[i]);
    }

    let has_connected = gaps.iter().any(|&g| g == 1);
    let has_one_gap = gaps.iter().any(|&g| g == 2);

    let straight_draw = has_straight_draw(&values);
    let flush_draw = max_suit >= 2 && board_cards.len() < 5;

    let connectedness = if has_connected && gaps.iter().filter(|&&g| g <= 2).count() >= 2 {
        Connectedness::Connected
    } else if has_connected || has_one_gap {
        Connectedness::SemiConnected
    } else {
        Connectedness::Disconnected
    };

    let mut wet_score: i32 = 0;
    if is_monotone {
        wet_score += 3;
    } else if is_two_tone {
        wet_score += 1;
    }
    if connectedness == Connectedness::Connected {
        wet_score += 2;
    } else if connectedness == Connectedness::SemiConnected {
        wet_score += 1;
    }
    if is_paired {
        wet_score -= 1;
    }

    let wetness = if wet_score >= 3 {
        Wetness::Wet
    } else if wet_score >= 1 {
        Wetness::Medium
    } else {
        Wetness::Dry
    };

    let mut draws = Vec::new();
    if flush_draw && is_two_tone {
        draws.push("flush draw".to_string());
    }
    if is_monotone {
        draws.push("flush complete / 4-flush".to_string());
    }
    if straight_draw {
        draws.push("straight draw".to_string());
    }
    if is_paired {
        draws.push("paired board".to_string());
    }

    let high_rank = value_to_rank(values[0]);
    let mut parts = Vec::new();
    if is_monotone {
        parts.push("monotone".to_string());
    } else if is_two_tone {
        parts.push("two-tone".to_string());
    } else {
        parts.push("rainbow".to_string());
    }
    parts.push(connectedness.to_string());
    if is_paired {
        parts.push("paired".to_string());
    }
    parts.push(format!("{}-high", high_rank));
    let category = parts.join(" ");

    Ok(BoardTexture {
        cards: board_cards.to_vec(),
        high_card: high_rank,
        is_paired,
        is_monotone,
        is_two_tone,
        is_rainbow,
        flush_draw_possible: flush_draw,
        straight_draw_possible: straight_draw,
        connectedness,
        wetness,
        category,
        draws,
    })
}

fn has_straight_draw(values: &[u8]) -> bool {
    let unique: Vec<u8> = {
        let mut s: Vec<u8> = values.iter().copied().collect::<HashSet<_>>().into_iter().collect();
        s.sort_unstable();
        s
    };

    for i in 0..unique.len() {
        let window_count = unique.iter().filter(|&&v| v >= unique[i] && v <= unique[i] + 4).count();
        if window_count >= 3 {
            return true;
        }
    }

    // Ace-low potential
    if unique.contains(&14) {
        let mut low_window: Vec<u8> = unique.iter().filter(|&&v| v <= 5).copied().collect();
        low_window.push(1); // ace as 1
        if low_window.len() >= 3 {
            return true;
        }
    }

    false
}

fn value_to_rank(value: u8) -> char {
    match value {
        2 => '2',
        3 => '3',
        4 => '4',
        5 => '5',
        6 => '6',
        7 => '7',
        8 => '8',
        9 => '9',
        10 => 'T',
        11 => 'J',
        12 => 'Q',
        13 => 'K',
        14 => 'A',
        _ => '?',
    }
}

pub struct CBetRecommendation {
    pub should_cbet: bool,
    pub frequency: f64,
    pub sizing: String,
    pub reasoning: String,
}

pub fn cbet_recommendation(
    board_texture: &BoardTexture,
    position: &str,
    spr_value: f64,
    multiway: bool,
) -> CBetRecommendation {
    let _ = spr_value; // matching Python signature

    if multiway {
        if board_texture.wetness == Wetness::Dry {
            return CBetRecommendation {
                should_cbet: true,
                frequency: 0.4,
                sizing: "33% pot".to_string(),
                reasoning: "Dry board multiway \u{2014} small sizing, lower frequency".to_string(),
            };
        }
        return CBetRecommendation {
            should_cbet: false,
            frequency: 0.2,
            sizing: "50% pot".to_string(),
            reasoning: "Wet board multiway \u{2014} check most hands, bet selectively".to_string(),
        };
    }

    let ip = position.to_uppercase() == "IP";

    if board_texture.wetness == Wetness::Dry {
        if ip {
            return CBetRecommendation {
                should_cbet: true,
                frequency: 0.7,
                sizing: "33% pot".to_string(),
                reasoning: "Dry board IP \u{2014} high frequency small c-bet".to_string(),
            };
        }
        return CBetRecommendation {
            should_cbet: true,
            frequency: 0.5,
            sizing: "33% pot".to_string(),
            reasoning: "Dry board OOP \u{2014} moderate frequency small c-bet".to_string(),
        };
    }

    if board_texture.wetness == Wetness::Wet {
        if ip {
            return CBetRecommendation {
                should_cbet: true,
                frequency: 0.5,
                sizing: "66-75% pot".to_string(),
                reasoning: "Wet board IP \u{2014} polarized sizing, moderate frequency".to_string(),
            };
        }
        return CBetRecommendation {
            should_cbet: true,
            frequency: 0.35,
            sizing: "66-75% pot".to_string(),
            reasoning: "Wet board OOP \u{2014} selective, larger sizing".to_string(),
        };
    }

    // Medium
    if ip {
        CBetRecommendation {
            should_cbet: true,
            frequency: 0.6,
            sizing: "50% pot".to_string(),
            reasoning: "Medium texture IP \u{2014} balanced frequency and sizing".to_string(),
        }
    } else {
        CBetRecommendation {
            should_cbet: true,
            frequency: 0.45,
            sizing: "50% pot".to_string(),
            reasoning: "Medium texture OOP \u{2014} moderate sizing".to_string(),
        }
    }
}

pub fn bet_sizing(
    board_texture: &BoardTexture,
    spr_value: f64,
    street: &str,
    polarized: bool,
) -> String {
    if polarized {
        if street == "river" {
            return "75-125% pot".to_string();
        }
        return "66-75% pot".to_string();
    }

    if spr_value <= 4.0 {
        return "33-50% pot (low SPR \u{2014} pot commitment)".to_string();
    }

    if street == "flop" {
        if board_texture.wetness == Wetness::Dry {
            return "25-33% pot".to_string();
        }
        if board_texture.wetness == Wetness::Wet {
            return "66-75% pot".to_string();
        }
        return "50% pot".to_string();
    }

    if street == "turn" {
        if board_texture.wetness == Wetness::Dry {
            return "50% pot".to_string();
        }
        return "66-75% pot".to_string();
    }

    // river
    "66-75% pot".to_string()
}

pub struct StreetStrategy {
    pub action: String,
    pub sizing: String,
    pub reasoning: String,
    pub hand_strength: String,
}

pub fn street_strategy(
    hand_strength: &str,
    board_texture: &BoardTexture,
    pot: f64,
    stack: f64,
    position: &str,
    street: &str,
) -> StreetStrategy {
    let spr_val = if pot > 0.0 { stack / pot } else { 10.0 };

    match hand_strength {
        "nuts" | "very_strong" => {
            if spr_val <= 4.0 {
                StreetStrategy {
                    action: "BET".to_string(),
                    sizing: "all-in or 66-100% pot".to_string(),
                    reasoning: "Low SPR with strong hand \u{2014} build pot for stacks".to_string(),
                    hand_strength: hand_strength.to_string(),
                }
            } else {
                let sizing = bet_sizing(board_texture, spr_val, street, true);
                StreetStrategy {
                    action: "BET".to_string(),
                    sizing,
                    reasoning: "Strong hand \u{2014} value bet".to_string(),
                    hand_strength: hand_strength.to_string(),
                }
            }
        }
        "strong" => {
            let sizing = bet_sizing(board_texture, spr_val, street, false);
            let reasoning = if board_texture.wetness == Wetness::Wet {
                "Strong hand on wet board \u{2014} protect equity"
            } else {
                "Strong hand \u{2014} standard value"
            };
            StreetStrategy {
                action: "BET".to_string(),
                sizing,
                reasoning: reasoning.to_string(),
                hand_strength: hand_strength.to_string(),
            }
        }
        "medium" => {
            if position.to_uppercase() == "IP" {
                StreetStrategy {
                    action: "CHECK/BET".to_string(),
                    sizing: "50% pot if betting".to_string(),
                    reasoning: "Medium hand IP \u{2014} pot control or thin value".to_string(),
                    hand_strength: hand_strength.to_string(),
                }
            } else {
                StreetStrategy {
                    action: "CHECK".to_string(),
                    sizing: "-".to_string(),
                    reasoning: "Medium hand OOP \u{2014} pot control".to_string(),
                    hand_strength: hand_strength.to_string(),
                }
            }
        }
        "draw" => {
            if board_texture.wetness == Wetness::Wet && position.to_uppercase() == "IP" {
                let sizing = bet_sizing(board_texture, spr_val, street, false);
                StreetStrategy {
                    action: "BET (semi-bluff)".to_string(),
                    sizing,
                    reasoning: "Draw IP \u{2014} semi-bluff for fold equity + equity".to_string(),
                    hand_strength: hand_strength.to_string(),
                }
            } else {
                StreetStrategy {
                    action: "CHECK/CALL".to_string(),
                    sizing: "-".to_string(),
                    reasoning: "Draw \u{2014} realize equity cheaply".to_string(),
                    hand_strength: hand_strength.to_string(),
                }
            }
        }
        "bluff" => {
            let freq = if stack > 0.0 {
                1.0 - (pot / (pot + stack))
            } else {
                0.3
            };
            let sizing = bet_sizing(board_texture, spr_val, street, true);
            StreetStrategy {
                action: "BET (bluff)".to_string(),
                sizing,
                reasoning: format!("Bluff \u{2014} need ~{:.0}% fold equity to profit", freq * 100.0),
                hand_strength: hand_strength.to_string(),
            }
        }
        _ => StreetStrategy {
            action: "CHECK/FOLD".to_string(),
            sizing: "-".to_string(),
            reasoning: "Weak hand \u{2014} give up without equity".to_string(),
            hand_strength: hand_strength.to_string(),
        },
    }
}
