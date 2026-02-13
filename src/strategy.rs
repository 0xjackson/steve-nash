//! Strategy lookup engine — queries solver output to answer:
//! "Given this hand + position + board, what are the GTO action frequencies?"

use crate::flop_solver::{FlopSolverConfig, FlopSolution, solve_flop};
use crate::preflop_solver::{Position, PreflopSolution, PreflopSpotResult};
use crate::river_solver::{RiverSolverConfig, RiverSolution, solve_river};
use crate::turn_solver::{TurnSolverConfig, TurnSolution, solve_turn};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

pub struct StrategyEngine {
    preflop: Option<PreflopSolution>,
    pub stack_bb: f64,
}

pub struct StrategyResult {
    pub actions: Vec<String>,
    pub frequencies: Vec<f64>,
    pub source: StrategySource,
}

#[derive(Debug, Clone, PartialEq)]
pub enum StrategySource {
    Cached,
    SolvedOnDemand,
    NotInRange,
}

/// Standard pot type for postflop solving.
#[derive(Debug, Clone, Copy)]
pub enum PotType {
    /// Single raised pot: 2.5bb open + BB call + blinds = 6bb, 97bb effective
    Srp,
    /// 3-bet pot: ~20bb pot, ~80bb effective
    ThreeBet,
    /// 4-bet pot: ~44bb pot, ~56bb effective
    FourBet,
}

impl PotType {
    pub fn pot_and_stack(&self) -> (f64, f64) {
        match self {
            PotType::Srp => (6.0, 97.0),
            PotType::ThreeBet => (20.0, 80.0),
            PotType::FourBet => (44.0, 56.0),
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            PotType::Srp => "SRP",
            PotType::ThreeBet => "3BP",
            PotType::FourBet => "4BP",
        }
    }
}

// ---------------------------------------------------------------------------
// Default villain positions
// ---------------------------------------------------------------------------

/// Default villain for a given hero position (most common matchup).
pub fn default_villain(hero: Position) -> Position {
    match hero {
        Position::BTN => Position::BB,
        Position::CO => Position::BB,
        Position::HJ => Position::BB,
        Position::UTG => Position::BB,
        Position::SB => Position::BB,
        Position::BB => Position::BTN,
    }
}

// ---------------------------------------------------------------------------
// Range derivation from preflop solver
// ---------------------------------------------------------------------------

const RANGE_THRESHOLD: f64 = 0.05;

/// Derive the opening range for a position from a solved preflop spot.
/// Returns hand notations (e.g., "AKs", "QQ") played >threshold frequency.
pub fn derive_opening_range(spot: &PreflopSpotResult, threshold: f64) -> Vec<String> {
    let mut hands = Vec::new();
    for i in 0..spot.open_strategy.len() {
        if spot.open_strategy[i] > threshold {
            hands.push(crate::game_tree::bucket_to_hand(i));
        }
    }
    hands
}

/// Derive the defending range (3-bet + call) for a responder from a solved preflop spot.
pub fn derive_defending_range(spot: &PreflopSpotResult, threshold: f64) -> Vec<String> {
    let mut hands = Vec::new();
    for i in 0..spot.vs_open_3bet.len() {
        let total = spot.vs_open_3bet[i] + spot.vs_open_call[i];
        if total > threshold {
            hands.push(crate::game_tree::bucket_to_hand(i));
        }
    }
    hands
}

// ---------------------------------------------------------------------------
// Combo lookup
// ---------------------------------------------------------------------------

/// Find the index of a specific hand combo (e.g., "AhQd") in a combo list.
/// Checks both orderings (AhQd and QdAh).
pub fn find_combo_index(hand_str: &str, combo_list: &[String]) -> Option<usize> {
    if hand_str.len() != 4 {
        return None;
    }
    let card1 = &hand_str[..2];
    let card2 = &hand_str[2..];
    let forward = format!("{}{}", card1, card2);
    let reverse = format!("{}{}", card2, card1);

    combo_list
        .iter()
        .position(|c| c == &forward || c == &reverse)
}

// ---------------------------------------------------------------------------
// StrategyEngine
// ---------------------------------------------------------------------------

impl StrategyEngine {
    pub fn new(stack_bb: f64) -> Self {
        // Try loading preflop solution
        let preflop = PreflopSolution::load("6max", stack_bb, 0.0).ok();
        StrategyEngine {
            preflop,
            stack_bb,
        }
    }

    pub fn has_preflop(&self) -> bool {
        self.preflop.is_some()
    }

    /// Query preflop strategy for a hand at a given position.
    /// Returns action frequencies from the solved preflop tree.
    pub fn query_preflop(
        &self,
        hand: &str,
        hero: Position,
        villain: Option<Position>,
    ) -> Option<StrategyResult> {
        let solution = self.preflop.as_ref()?;
        let bucket = crate::game_tree::hand_to_bucket(hand)?;

        match villain {
            None => {
                // RFI — opener's decision at node 100
                let spot = solution.spots.iter().find(|s| s.opener == hero)?;
                let open_freq = spot.open_strategy[bucket];
                let fold_freq = 1.0 - open_freq;
                Some(StrategyResult {
                    actions: vec!["RAISE 2.5bb".to_string(), "FOLD".to_string()],
                    frequencies: vec![open_freq, fold_freq],
                    source: StrategySource::Cached,
                })
            }
            Some(villain_pos) => {
                let hero_order = preflop_open_order(hero);
                let villain_order = preflop_open_order(villain_pos);

                if hero_order > villain_order {
                    // Villain opened, hero responds
                    let spot = solution.find_spot(villain_pos, hero)?;
                    let threebet = spot.vs_open_3bet[bucket];
                    let call = spot.vs_open_call[bucket];
                    let fold = (1.0 - threebet - call).max(0.0);
                    Some(StrategyResult {
                        actions: vec![
                            "3-BET".to_string(),
                            "CALL".to_string(),
                            "FOLD".to_string(),
                        ],
                        frequencies: vec![threebet, call, fold],
                        source: StrategySource::Cached,
                    })
                } else {
                    // Hero opened, villain 3-bet
                    let spot = solution.find_spot(hero, villain_pos)?;
                    let fourbet = spot.vs_3bet_4bet[bucket];
                    let call = spot.vs_3bet_call[bucket];
                    let fold = (1.0 - fourbet - call).max(0.0);
                    Some(StrategyResult {
                        actions: vec![
                            "4-BET".to_string(),
                            "CALL".to_string(),
                            "FOLD".to_string(),
                        ],
                        frequencies: vec![fourbet, call, fold],
                        source: StrategySource::Cached,
                    })
                }
            }
        }
    }

    /// Query postflop strategy for a specific hand on a given board.
    /// Will solve on-demand if no cached solution exists.
    pub fn query_postflop(
        &mut self,
        hand: &str,
        hero: Position,
        villain: Position,
        board: &str,
        pot: f64,
        stack: f64,
        iterations: usize,
    ) -> Result<StrategyResult, String> {
        let board_len = board.len();
        let hero_side = if hero.is_ip_vs(&villain) { "IP" } else { "OOP" };

        // Derive ranges from preflop solution
        let (oop_range, ip_range) = self.derive_postflop_ranges(hero, villain)?;
        let oop_str = oop_range.join(",");
        let ip_str = ip_range.join(",");

        match board_len {
            6 => self.query_flop(hand, hero_side, board, &oop_str, &ip_str, pot, stack, iterations),
            8 => self.query_turn(hand, hero_side, board, &oop_str, &ip_str, pot, stack, iterations),
            10 => self.query_river(hand, hero_side, board, &oop_str, &ip_str, pot, stack, iterations),
            _ => Err(format!("Invalid board length: {} chars (expected 6, 8, or 10)", board_len)),
        }
    }

    /// Derive OOP and IP ranges for a postflop spot from preflop solution.
    fn derive_postflop_ranges(
        &self,
        hero: Position,
        villain: Position,
    ) -> Result<(Vec<String>, Vec<String>), String> {
        let solution = self.preflop.as_ref().ok_or_else(|| {
            "No preflop solution loaded. Run `gto solve preflop` first.".to_string()
        })?;

        // Determine who opened and who defended
        let hero_order = preflop_open_order(hero);
        let villain_order = preflop_open_order(villain);

        let (opener, responder) = if hero_order < villain_order {
            (hero, villain)
        } else {
            (villain, hero)
        };

        let spot = solution
            .find_spot(opener, responder)
            .ok_or_else(|| format!("No preflop spot found for {} vs {}", opener, responder))?;

        let opener_range = derive_opening_range(spot, RANGE_THRESHOLD);
        let responder_range = derive_defending_range(spot, RANGE_THRESHOLD);

        if opener_range.is_empty() || responder_range.is_empty() {
            return Err("Derived ranges are empty".to_string());
        }

        // OOP = whoever acts first postflop
        if opener.is_ip_vs(&responder) {
            // Opener is IP, responder is OOP
            Ok((responder_range, opener_range))
        } else {
            Ok((opener_range, responder_range))
        }
    }

    fn query_flop(
        &self,
        hand: &str,
        hero_side: &str,
        board: &str,
        oop_range: &str,
        ip_range: &str,
        pot: f64,
        stack: f64,
        iterations: usize,
    ) -> Result<StrategyResult, String> {
        // Try cache first (with position info in key)
        if let Some(solution) = FlopSolution::load_cache(board, pot, stack) {
            return lookup_in_flop_solution(&solution, hand, hero_side);
        }

        // Solve on-demand
        eprintln!("  Solving flop {} (this may take 1-4 min)...", board);
        let config = FlopSolverConfig::new(board, oop_range, ip_range, pot, stack, iterations)?;
        let solution = solve_flop(&config);
        solution.save_cache();

        lookup_in_flop_solution(&solution, hand, hero_side)
    }

    fn query_turn(
        &self,
        hand: &str,
        hero_side: &str,
        board: &str,
        oop_range: &str,
        ip_range: &str,
        pot: f64,
        stack: f64,
        iterations: usize,
    ) -> Result<StrategyResult, String> {
        if let Some(solution) = TurnSolution::load_cache(board, pot, stack) {
            return lookup_in_turn_solution(&solution, hand, hero_side);
        }

        eprintln!("  Solving turn {} (this may take 15-45s)...", board);
        let config = TurnSolverConfig::new(board, oop_range, ip_range, pot, stack, iterations)?;
        let solution = solve_turn(&config);
        solution.save_cache();

        lookup_in_turn_solution(&solution, hand, hero_side)
    }

    fn query_river(
        &self,
        hand: &str,
        hero_side: &str,
        board: &str,
        oop_range: &str,
        ip_range: &str,
        pot: f64,
        stack: f64,
        iterations: usize,
    ) -> Result<StrategyResult, String> {
        if let Some(solution) = RiverSolution::load_cache(board, pot, stack) {
            return lookup_in_river_solution(&solution, hand, hero_side);
        }

        eprintln!("  Solving river {} (this may take 1-5s)...", board);
        let config = RiverSolverConfig::new(board, oop_range, ip_range, pot, stack, iterations)?;
        let solution = solve_river(&config);
        solution.save_cache();

        lookup_in_river_solution(&solution, hand, hero_side)
    }
}

// ---------------------------------------------------------------------------
// Solution lookup helpers
// ---------------------------------------------------------------------------

fn lookup_in_flop_solution(
    solution: &FlopSolution,
    hand: &str,
    hero_side: &str,
) -> Result<StrategyResult, String> {
    let combos = if hero_side == "OOP" {
        &solution.oop_combos
    } else {
        &solution.ip_combos
    };

    let combo_idx = match find_combo_index(hand, combos) {
        Some(idx) => idx,
        None => {
            return Ok(StrategyResult {
                actions: vec![],
                frequencies: vec![],
                source: StrategySource::NotInRange,
            });
        }
    };

    // Find first strategy node matching hero's side
    for strat in &solution.strategies {
        if strat.player == hero_side && combo_idx < strat.frequencies.len() {
            return Ok(StrategyResult {
                actions: strat.actions.clone(),
                frequencies: strat.frequencies[combo_idx].clone(),
                source: StrategySource::Cached,
            });
        }
    }

    Err("No strategy found for hero's side at root node".to_string())
}

fn lookup_in_turn_solution(
    solution: &TurnSolution,
    hand: &str,
    hero_side: &str,
) -> Result<StrategyResult, String> {
    let combos = if hero_side == "OOP" {
        &solution.oop_combos
    } else {
        &solution.ip_combos
    };

    let combo_idx = match find_combo_index(hand, combos) {
        Some(idx) => idx,
        None => {
            return Ok(StrategyResult {
                actions: vec![],
                frequencies: vec![],
                source: StrategySource::NotInRange,
            });
        }
    };

    for strat in &solution.strategies {
        if strat.player == hero_side && combo_idx < strat.frequencies.len() {
            return Ok(StrategyResult {
                actions: strat.actions.clone(),
                frequencies: strat.frequencies[combo_idx].clone(),
                source: StrategySource::Cached,
            });
        }
    }

    Err("No strategy found for hero's side at root node".to_string())
}

fn lookup_in_river_solution(
    solution: &RiverSolution,
    hand: &str,
    hero_side: &str,
) -> Result<StrategyResult, String> {
    let combos = if hero_side == "OOP" {
        &solution.oop_combos
    } else {
        &solution.ip_combos
    };

    let combo_idx = match find_combo_index(hand, combos) {
        Some(idx) => idx,
        None => {
            return Ok(StrategyResult {
                actions: vec![],
                frequencies: vec![],
                source: StrategySource::NotInRange,
            });
        }
    };

    for strat in &solution.strategies {
        if strat.player == hero_side && combo_idx < strat.frequencies.len() {
            return Ok(StrategyResult {
                actions: strat.actions.clone(),
                frequencies: strat.frequencies[combo_idx].clone(),
                source: StrategySource::Cached,
            });
        }
    }

    Err("No strategy found for hero's side at root node".to_string())
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn preflop_open_order(pos: Position) -> usize {
    match pos {
        Position::UTG => 0,
        Position::HJ => 1,
        Position::CO => 2,
        Position::BTN => 3,
        Position::SB => 4,
        Position::BB => 5,
    }
}

/// Format a hand string with unicode suit symbols for display.
/// "AhQd" -> "A♥Q♦"
pub fn pretty_hand(hand: &str) -> String {
    if hand.len() != 4 {
        return hand.to_string();
    }
    let chars: Vec<char> = hand.chars().collect();
    let r1 = chars[0];
    let s1 = suit_symbol(chars[1]);
    let r2 = chars[2];
    let s2 = suit_symbol(chars[3]);
    format!("{}{}{}{}", r1, s1, r2, s2)
}

/// Format a board string with unicode suit symbols for display.
/// "Ks9d4c" -> "K♠ 9♦ 4♣"
pub fn pretty_board(board: &str) -> String {
    let chars: Vec<char> = board.chars().collect();
    let mut result = Vec::new();
    let mut i = 0;
    while i + 1 < chars.len() {
        let rank = chars[i];
        let suit = suit_symbol(chars[i + 1]);
        result.push(format!("{}{}", rank, suit));
        i += 2;
    }
    result.join(" ")
}

fn suit_symbol(c: char) -> &'static str {
    match c {
        's' => "\u{2660}",
        'h' => "\u{2665}",
        'd' => "\u{2666}",
        'c' => "\u{2663}",
        _ => "?",
    }
}

/// Detect the street from board string length.
pub fn detect_street(board: &str) -> &'static str {
    match board.len() {
        0 => "Preflop",
        6 => "Flop",
        8 => "Turn",
        10 => "River",
        _ => "Unknown",
    }
}

/// Format strategy result as a display string.
/// "→ CHECK (45%), BET 33% (30%), BET 75% (25%)"
pub fn format_strategy(result: &StrategyResult) -> String {
    if result.source == StrategySource::NotInRange {
        return "Hand not in range for this spot".to_string();
    }

    let mut pairs: Vec<(&str, f64)> = result
        .actions
        .iter()
        .zip(&result.frequencies)
        .map(|(a, f)| (a.as_str(), *f))
        .collect();

    // Sort by frequency descending
    pairs.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

    // Filter out <1% actions
    let parts: Vec<String> = pairs
        .iter()
        .filter(|(_, f)| *f > 0.01)
        .map(|(action, freq)| format!("{} ({:.0}%)", action, freq * 100.0))
        .collect();

    if parts.is_empty() {
        "No significant actions".to_string()
    } else {
        format!("\u{2192} {}", parts.join(", "))
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_find_combo_index() {
        let combos = vec![
            "AhKs".to_string(),
            "QdJc".to_string(),
            "TsTs".to_string(),
        ];
        assert_eq!(find_combo_index("AhKs", &combos), Some(0));
        assert_eq!(find_combo_index("KsAh", &combos), Some(0));
        assert_eq!(find_combo_index("QdJc", &combos), Some(1));
        assert_eq!(find_combo_index("JcQd", &combos), Some(1));
        assert_eq!(find_combo_index("AcKd", &combos), None);
    }

    #[test]
    fn test_pretty_hand() {
        assert_eq!(pretty_hand("AhQd"), "A\u{2665}Q\u{2666}");
        assert_eq!(pretty_hand("KsTs"), "K\u{2660}T\u{2660}");
    }

    #[test]
    fn test_pretty_board() {
        assert_eq!(pretty_board("Ks9d4c"), "K\u{2660} 9\u{2666} 4\u{2663}");
    }

    #[test]
    fn test_detect_street() {
        assert_eq!(detect_street(""), "Preflop");
        assert_eq!(detect_street("Ks9d4c"), "Flop");
        assert_eq!(detect_street("Ks9d4c7h"), "Turn");
        assert_eq!(detect_street("Ks9d4c7hQc"), "River");
    }

    #[test]
    fn test_default_villain() {
        assert_eq!(default_villain(Position::BTN), Position::BB);
        assert_eq!(default_villain(Position::BB), Position::BTN);
        assert_eq!(default_villain(Position::CO), Position::BB);
    }

    #[test]
    fn test_pot_type() {
        let (pot, stack) = PotType::Srp.pot_and_stack();
        assert!((pot - 6.0).abs() < 0.01);
        assert!((stack - 97.0).abs() < 0.01);
    }

    #[test]
    fn test_format_strategy() {
        let result = StrategyResult {
            actions: vec!["CHECK".to_string(), "BET 33%".to_string(), "BET 75%".to_string()],
            frequencies: vec![0.45, 0.30, 0.25],
            source: StrategySource::Cached,
        };
        let formatted = format_strategy(&result);
        assert!(formatted.contains("CHECK"));
        assert!(formatted.contains("45%"));
        assert!(formatted.contains("BET 33%"));
    }

    #[test]
    fn test_format_strategy_not_in_range() {
        let result = StrategyResult {
            actions: vec![],
            frequencies: vec![],
            source: StrategySource::NotInRange,
        };
        assert!(format_strategy(&result).contains("not in range"));
    }
}
