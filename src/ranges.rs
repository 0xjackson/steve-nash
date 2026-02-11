use crate::cards::{hand_combos, Card, RANKS_STR};
use crate::error::{GtoError, GtoResult};

pub const HAND_RANKING: &[&str] = &[
    "AA", "KK", "QQ", "AKs", "JJ", "AQs", "KQs", "AJs", "KJs", "TT",
    "AKo", "ATs", "QJs", "KTs", "QTs", "JTs", "99", "AQo", "A9s", "KQo",
    "K9s", "T9s", "J9s", "Q9s", "A8s", "88", "A5s", "A7s", "A4s", "A6s",
    "A3s", "K8s", "T8s", "A2s", "98s", "J8s", "77", "Q8s", "K7s", "AJo",
    "87s", "66", "K6s", "ATo", "97s", "76s", "T7s", "K5s", "ATo", "55",
    "J7s", "86s", "KJo", "65s", "Q7s", "K4s", "K3s", "K2s", "96s", "44",
    "QJo", "75s", "54s", "A9o", "T6s", "KTo", "J6s", "Q6s", "33", "85s",
    "64s", "QTo", "22", "53s", "JTo", "K9o", "J9o", "T9o", "Q9o", "74s",
    "43s", "A8o", "A5o", "A7o", "A4o", "A6o", "A3o", "95s", "63s", "A2o",
    "52s", "84s", "42s", "T8o", "98o", "J8o", "Q8o", "73s", "87o", "32s",
    "62s", "97o", "76o", "K8o", "86o", "65o", "94s", "93s", "92s", "T7o",
    "54o", "83s", "75o", "82s", "K7o", "K6o", "72s", "96o", "J7o", "K5o",
    "T6o", "K4o", "K3o", "K2o", "85o", "Q7o", "64o", "53o", "J6o", "Q6o",
    "Q5o", "Q4o", "Q3o", "Q2o", "74o", "43o", "95o", "63o", "84o", "42o",
    "T5o", "T4o", "T3o", "T2o", "52o", "J5o", "J4o", "J3o", "J2o", "73o",
    "32o", "62o", "94o", "93o", "92o", "83o", "82o", "72o",
];

pub fn combo_count(notation: &str) -> u32 {
    let chars: Vec<char> = notation.chars().collect();
    if chars.len() == 2 && chars[0] == chars[1] {
        return 6;
    }
    if chars.len() == 3 {
        if chars[2] == 's' {
            return 4;
        }
        if chars[2] == 'o' {
            return 12;
        }
    }
    0
}

pub fn parse_range(range_str: &str) -> Vec<String> {
    let mut hands = std::collections::HashSet::new();
    for part in range_str.replace(' ', "").split(',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        if part.ends_with('+') {
            for h in expand_plus(&part[..part.len() - 1]) {
                hands.insert(h);
            }
        } else if part.contains('-') && part.len() > 3 {
            for h in expand_dash(part) {
                hands.insert(h);
            }
        } else {
            hands.insert(part.to_string());
        }
    }
    let mut result: Vec<String> = hands.into_iter().collect();
    result.sort_by_key(|h| hand_strength_index(h));
    result
}

fn expand_plus(base: &str) -> Vec<String> {
    let chars: Vec<char> = base.chars().collect();

    // Pair: "TT+"
    if chars.len() == 2 && chars[0] == chars[1] {
        if let Some(rank_idx) = RANKS_STR.find(chars[0]) {
            let ranks: Vec<char> = RANKS_STR.chars().collect();
            return (rank_idx..ranks.len())
                .map(|i| format!("{}{}", ranks[i], ranks[i]))
                .collect();
        }
        return vec![base.to_string()];
    }

    // Suited/offsuit: "ATs+"
    if chars.len() == 3 {
        let high = chars[0];
        let low = chars[1];
        let kind = chars[2];
        if let (Some(low_idx), Some(high_idx)) = (RANKS_STR.find(low), RANKS_STR.find(high)) {
            let ranks: Vec<char> = RANKS_STR.chars().collect();
            return (low_idx..high_idx)
                .map(|i| format!("{}{}{}", high, ranks[i], kind))
                .collect();
        }
    }

    vec![base.to_string()]
}

fn expand_dash(range_str: &str) -> Vec<String> {
    let parts: Vec<&str> = range_str.split('-').collect();
    if parts.len() != 2 {
        return vec![range_str.to_string()];
    }

    let (start, end) = (parts[0], parts[1]);
    let start_chars: Vec<char> = start.chars().collect();
    let end_chars: Vec<char> = end.chars().collect();
    let ranks: Vec<char> = RANKS_STR.chars().collect();

    // Pair range: "77-TT"
    if start_chars.len() == 2
        && end_chars.len() == 2
        && start_chars[0] == start_chars[1]
        && end_chars[0] == end_chars[1]
    {
        if let (Some(si), Some(ei)) = (RANKS_STR.find(start_chars[0]), RANKS_STR.find(end_chars[0]))
        {
            let lo = si.min(ei);
            let hi = si.max(ei);
            return (lo..=hi).map(|i| format!("{}{}", ranks[i], ranks[i])).collect();
        }
    }

    // Suited/offsuit range: "KTs-KQs"
    if start_chars.len() == 3
        && end_chars.len() == 3
        && start_chars[0] == end_chars[0]
        && start_chars[2] == end_chars[2]
    {
        let high = start_chars[0];
        let kind = start_chars[2];
        if let (Some(si), Some(ei)) = (RANKS_STR.find(start_chars[1]), RANKS_STR.find(end_chars[1]))
        {
            let lo = si.min(ei);
            let hi = si.max(ei);
            return (lo..=hi)
                .map(|i| format!("{}{}{}", high, ranks[i], kind))
                .collect();
        }
    }

    vec![range_str.to_string()]
}

fn hand_strength_index(hand: &str) -> usize {
    HAND_RANKING
        .iter()
        .position(|&h| h == hand)
        .unwrap_or(HAND_RANKING.len())
}

pub fn range_from_top_pct(pct: f64) -> GtoResult<Vec<String>> {
    if pct <= 0.0 || pct > 100.0 {
        return Err(GtoError::InvalidValue(
            "Percentage must be between 0 and 100".to_string(),
        ));
    }
    let total_combos = 1326.0;
    let target = total_combos * (pct / 100.0);
    let mut result = Vec::new();
    let mut running = 0u32;
    for &hand in HAND_RANKING {
        let count = combo_count(hand);
        if running + count > target as u32 && running > 0 {
            break;
        }
        result.push(hand.to_string());
        running += count;
        if running as f64 >= target {
            break;
        }
    }
    Ok(result)
}

pub fn total_combos(hands: &[String]) -> u32 {
    hands.iter().map(|h| combo_count(h)).sum()
}

pub fn total_combos_strs(hands: &[&str]) -> u32 {
    hands.iter().map(|h| combo_count(h)).sum()
}

pub fn range_pct(hands: &[String]) -> f64 {
    total_combos(hands) as f64 / 1326.0 * 100.0
}

pub fn range_pct_strs(hands: &[&str]) -> f64 {
    total_combos_strs(hands) as f64 / 1326.0 * 100.0
}

pub fn blockers_remove(villain_range: &[String], hero_cards: &[Card]) -> Vec<String> {
    let mut result = Vec::new();
    for hand in villain_range {
        if let Ok(combos) = hand_combos(hand) {
            let remaining: Vec<_> = combos
                .into_iter()
                .filter(|(c1, c2)| !hero_cards.contains(c1) && !hero_cards.contains(c2))
                .collect();
            if !remaining.is_empty() {
                result.push(hand.clone());
            }
        }
    }
    result
}

pub fn blocked_combos(hand_notation: &str, hero_cards: &[Card]) -> GtoResult<u32> {
    let combos = hand_combos(hand_notation)?;
    let remaining = combos
        .iter()
        .filter(|(c1, c2)| !hero_cards.contains(c1) && !hero_cards.contains(c2))
        .count();
    Ok(combo_count(hand_notation) - remaining as u32)
}
