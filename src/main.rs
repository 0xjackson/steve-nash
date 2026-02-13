mod batch;
mod bucketing;
mod card_encoding;
mod cards;
mod cfr;
mod cli;
mod display;
mod equity;
mod error;
mod flat_cfr;
mod flop_enumerator;
mod flop_solver;
mod game_tree;
mod hand_evaluator;
mod lookup_eval;
mod math_engine;
mod multiway;
mod play;
mod postflop;
mod postflop_tree;
mod preflop;
mod preflop_solver;
mod ranges;
mod river_solver;
mod strategy;
mod turn_solver;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let args = preprocess_args(args);
    cli::run_with_args(args);
}

/// Detect shorthand: `gto AhKs BTN Ks9d4c` → `gto query AhKs BTN Ks9d4c`
/// A hand looks like 4 chars where chars 1,3 are valid ranks and chars 2,4 are valid suits.
fn preprocess_args(args: Vec<String>) -> Vec<String> {
    if args.len() >= 3 && looks_like_hand(&args[1]) {
        let mut new_args = vec![args[0].clone(), "query".to_string()];
        new_args.extend_from_slice(&args[1..]);
        new_args
    } else {
        args
    }
}

fn looks_like_hand(s: &str) -> bool {
    if s.len() != 4 {
        return false;
    }
    let chars: Vec<char> = s.chars().collect();
    is_rank(chars[0]) && is_suit(chars[1]) && is_rank(chars[2]) && is_suit(chars[3])
}

fn is_rank(c: char) -> bool {
    matches!(c, '2'..='9' | 'T' | 'J' | 'Q' | 'K' | 'A')
}

fn is_suit(c: char) -> bool {
    matches!(c, 's' | 'h' | 'd' | 'c')
}
