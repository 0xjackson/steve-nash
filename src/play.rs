use std::collections::HashSet;
use std::io::{self, BufRead, Write};

use colored::Colorize;

use crate::cards::{parse_board, parse_card, simplify_hand, Card};
use crate::display::{board_display, equity_bar, styled_action};
use crate::equity::equity_vs_range;
use crate::hand_evaluator::{evaluate_hand, HandCategory, HandResult};
use crate::math_engine::{break_even_pct, spr as calc_spr, SprZone};
use crate::multiway::multiway_range_adjustment;
use crate::postflop::{analyze_board, street_strategy, Wetness};
use crate::preflop::{
    get_rfi_pct, get_rfi_range, preflop_action, positions_for,
};
use crate::preflop_solver::Position;
use crate::ranges::{blockers_remove, range_from_top_pct, HAND_RANKING};
use crate::strategy::{
    default_villain, detect_street, format_strategy, StrategyEngine, StrategySource,
};

// ---------------------------------------------------------------------------
// Position helpers
// ---------------------------------------------------------------------------

const POSITION_ORDER_6MAX: &[(&str, u8)] = &[
    ("SB", 0), ("BB", 1), ("UTG", 2), ("HJ", 3), ("CO", 4), ("BTN", 5),
];

const POSITION_ORDER_9MAX: &[(&str, u8)] = &[
    ("SB", 0), ("BB", 1), ("UTG", 2), ("UTG1", 3), ("UTG2", 4),
    ("MP", 5), ("HJ", 6), ("CO", 7), ("BTN", 8),
];

fn position_order(pos: &str, table_size: &str) -> u8 {
    let table = if table_size == "9max" {
        POSITION_ORDER_9MAX
    } else {
        POSITION_ORDER_6MAX
    };
    table.iter().find(|(p, _)| *p == pos).map(|(_, o)| *o).unwrap_or(0)
}

pub fn is_in_position(hero_pos: &str, villain_pos: &str, table_size: &str) -> bool {
    position_order(hero_pos, table_size) > position_order(villain_pos, table_size)
}

pub fn explain_position(pos: &str) -> &'static str {
    match pos {
        "UTG" => "Under the Gun \u{2014} first to act, play tight",
        "UTG1" => "UTG+1 \u{2014} early position, play tight",
        "UTG2" => "UTG+2 \u{2014} early position, play tight",
        "MP" => "Middle Position \u{2014} slightly wider than early positions",
        "HJ" => "Hijack \u{2014} one before the Cutoff, starting to open wider",
        "CO" => "Cutoff \u{2014} strong position, wide opening range",
        "BTN" => "Button \u{2014} best seat, you act last after the flop",
        "SB" => "Small Blind \u{2014} worst postflop position, act first",
        "BB" => "Big Blind \u{2014} last to act preflop, defend wide",
        _ => "Unknown position",
    }
}

// ---------------------------------------------------------------------------
// Hand strength classifier
// ---------------------------------------------------------------------------

pub fn classify_hand_strength(
    hand_result: &HandResult,
    hole_cards: &[Card],
    board: &[Card],
    equity: f64,
) -> &'static str {
    match hand_result.category {
        HandCategory::RoyalFlush | HandCategory::StraightFlush | HandCategory::FourOfAKind => {
            "nuts"
        }
        HandCategory::FullHouse => "very_strong",
        HandCategory::Flush | HandCategory::Straight => {
            if equity >= 0.65 {
                "very_strong"
            } else {
                "strong"
            }
        }
        HandCategory::ThreeOfAKind => {
            let is_pocket_pair = hole_cards.len() == 2 && hole_cards[0].rank == hole_cards[1].rank;
            if is_pocket_pair && board.iter().any(|c| c.rank == hole_cards[0].rank) {
                "very_strong"
            } else {
                let hero_ranks: HashSet<_> = hole_cards.iter().map(|c| c.rank).collect();
                let board_rank_counts = {
                    let mut counts = std::collections::HashMap::new();
                    for c in board {
                        *counts.entry(c.rank).or_insert(0u32) += 1;
                    }
                    counts
                };
                let is_set = hero_ranks
                    .iter()
                    .any(|r| board_rank_counts.get(r).copied().unwrap_or(0) == 1 && hole_cards.iter().filter(|c| c.rank == *r).count() == 2);
                if is_set { "very_strong" } else { "strong" }
            }
        }
        HandCategory::TwoPair => {
            if equity >= 0.55 { "strong" } else { "medium" }
        }
        HandCategory::OnePair => {
            let pair_rank_value = hand_result.kickers[0];
            let mut board_values: Vec<u8> = board.iter().map(|c| c.value()).collect();
            board_values.sort_unstable_by(|a, b| b.cmp(a));
            let hero_values: Vec<u8> = hole_cards.iter().map(|c| c.value()).collect();

            let is_top_pair = !board_values.is_empty() && pair_rank_value >= board_values[0];
            let kicker_value = hero_values
                .iter()
                .filter(|&&v| v != pair_rank_value)
                .max()
                .copied()
                .unwrap_or(0);

            if is_top_pair && kicker_value >= 12 {
                "strong"
            } else if is_top_pair {
                "medium"
            } else if board_values.len() > 1 && pair_rank_value >= board_values[1] {
                "medium"
            } else {
                "weak"
            }
        }
        HandCategory::HighCard => {
            if has_flush_draw(hole_cards, board) || has_straight_draw_hero(hole_cards, board) {
                "draw"
            } else if equity >= 0.35 {
                "medium"
            } else {
                "weak"
            }
        }
    }
}

fn has_flush_draw(hole_cards: &[Card], board: &[Card]) -> bool {
    let mut suit_counts = [0u32; 4];
    let mut hero_suits = [false; 4];
    for c in hole_cards {
        let idx = c.suit as usize;
        suit_counts[idx] += 1;
        hero_suits[idx] = true;
    }
    for c in board {
        suit_counts[c.suit as usize] += 1;
    }
    suit_counts
        .iter()
        .enumerate()
        .any(|(i, &count)| count >= 4 && hero_suits[i])
}

fn has_straight_draw_hero(hole_cards: &[Card], board: &[Card]) -> bool {
    let all_values: HashSet<u8> = hole_cards
        .iter()
        .chain(board.iter())
        .map(|c| c.value())
        .collect();
    let hero_values: HashSet<u8> = hole_cards.iter().map(|c| c.value()).collect();

    for start in 2u8..=10 {
        let window: Vec<u8> = all_values
            .iter()
            .filter(|&&v| v >= start && v <= start + 4)
            .copied()
            .collect();
        if window.len() >= 4 && window.iter().any(|v| hero_values.contains(v)) {
            return true;
        }
    }
    // Ace-low
    if all_values.contains(&14) {
        let mut low_vals: Vec<u8> = all_values.iter().filter(|&&v| v <= 5).copied().collect();
        low_vals.push(1);
        if low_vals.len() >= 4 {
            let hero_contributes = hero_values.contains(&14)
                || hero_values.iter().any(|v| *v <= 5);
            if hero_contributes {
                return true;
            }
        }
    }
    false
}

pub fn explain_hand_category(category: HandCategory) -> &'static str {
    match category {
        HandCategory::RoyalFlush => "Royal Flush \u{2014} the absolute best hand possible",
        HandCategory::StraightFlush => "Straight Flush \u{2014} nearly unbeatable",
        HandCategory::FourOfAKind => "Four of a Kind \u{2014} monster hand",
        HandCategory::FullHouse => "Full House \u{2014} very strong, hard to beat",
        HandCategory::Flush => "Flush \u{2014} strong, but watch for higher flushes",
        HandCategory::Straight => "Straight \u{2014} strong, but vulnerable to flushes and full houses",
        HandCategory::ThreeOfAKind => "Three of a Kind \u{2014} strong, good for value",
        HandCategory::TwoPair => "Two Pair \u{2014} decent but vulnerable on wet boards",
        HandCategory::OnePair => "One Pair \u{2014} solid but vulnerable",
        HandCategory::HighCard => "High Card \u{2014} usually not strong enough to bet for value",
    }
}

pub fn explain_board_texture(wetness: Wetness) -> &'static str {
    match wetness {
        Wetness::Dry => "DRY board \u{2014} few draws possible, made hands hold up",
        Wetness::Medium => "MEDIUM texture \u{2014} some draws possible, be aware of turn/river changes",
        Wetness::Wet => "WET board \u{2014} many draws possible, protect your hand or bet for value",
    }
}

pub fn explain_spr(zone: SprZone) -> &'static str {
    match zone {
        SprZone::Low => "Low SPR \u{2014} commit with top pair+, all-in pressure is standard",
        SprZone::Medium => "Medium SPR \u{2014} top pair is good for value, be careful about going all-in",
        SprZone::High => "High SPR \u{2014} need very strong hands to stack off, implied odds matter",
    }
}

pub fn explain_strength(strength: &str) -> &'static str {
    match strength {
        "nuts" => "You have the nuts or near-nuts \u{2014} extract maximum value",
        "very_strong" => "Very strong hand \u{2014} bet for value, build the pot",
        "strong" => "Strong hand \u{2014} bet for value and protection",
        "medium" => "Medium-strength hand \u{2014} control the pot, don't overcommit",
        "draw" => "Drawing hand \u{2014} you need to improve, consider semi-bluffing",
        "weak" => "Weak hand \u{2014} consider giving up or bluffing selectively",
        "bluff" => "Bluffing opportunity \u{2014} need fold equity to profit",
        _ => "Unknown strength",
    }
}

// ---------------------------------------------------------------------------
// Villain range estimator
// ---------------------------------------------------------------------------

pub fn estimate_villain_range(
    situation: &str,
    _hero_pos: &str,
    villain_pos: Option<&str>,
    hero_cards: &[Card],
    table_size: &str,
) -> Vec<String> {
    let villain_range = match situation {
        "RFI" => {
            range_from_top_pct(20.0).unwrap_or_default()
        }
        "vs_RFI" => {
            if let Some(vp) = villain_pos {
                let r = get_rfi_range(vp, table_size);
                if r.is_empty() {
                    range_from_top_pct(20.0).unwrap_or_default()
                } else {
                    r
                }
            } else {
                range_from_top_pct(20.0).unwrap_or_default()
            }
        }
        "vs_3bet" => {
            range_from_top_pct(7.0).unwrap_or_default()
        }
        "bb_defense" => {
            if let Some(vp) = villain_pos {
                let r = get_rfi_range(vp, table_size);
                if r.is_empty() {
                    range_from_top_pct(25.0).unwrap_or_default()
                } else {
                    r
                }
            } else {
                range_from_top_pct(25.0).unwrap_or_default()
            }
        }
        _ => range_from_top_pct(20.0).unwrap_or_default(),
    };

    let range = if villain_range.is_empty() {
        range_from_top_pct(20.0).unwrap_or_default()
    } else {
        villain_range
    };

    blockers_remove(&range, hero_cards)
}

// ---------------------------------------------------------------------------
// Input helpers
// ---------------------------------------------------------------------------

fn prompt(message: &str, default: Option<&str>, reader: &mut dyn BufRead, writer: &mut dyn Write) -> String {
    if let Some(d) = default {
        write!(writer, "{} [{}]: ", message, d).ok();
    } else {
        write!(writer, "{}: ", message).ok();
    }
    writer.flush().ok();

    let mut line = String::new();
    match reader.read_line(&mut line) {
        Ok(0) => "q".to_string(),
        Ok(_) => {
            let trimmed = line.trim().to_string();
            if trimmed.is_empty() {
                default.unwrap_or("").to_string()
            } else {
                trimmed
            }
        }
        Err(_) => "q".to_string(),
    }
}

fn prompt_menu(
    title: &str,
    options: &[&str],
    default_idx: usize,
    reader: &mut dyn BufRead,
    writer: &mut dyn Write,
) -> String {
    writeln!(writer, "\n  {}", title.bold()).ok();
    for (i, opt) in options.iter().enumerate() {
        let marker = if i == default_idx { " <" } else { "" };
        writeln!(writer, "    {}  {}{}", format!("{}.", i + 1).bold(), opt, marker.dimmed()).ok();
    }
    let answer = prompt("  Enter a number", Some(&format!("{}", default_idx + 1)), reader, writer);
    if answer.to_lowercase() == "q" {
        return "q".to_string();
    }
    let lower = answer.to_lowercase();
    for opt in options {
        if opt.to_lowercase() == lower || opt.to_lowercase().starts_with(&lower) {
            return opt.to_string();
        }
    }
    if let Ok(n) = answer.parse::<usize>() {
        if n >= 1 && n <= options.len() {
            return options[n - 1].to_string();
        }
    }
    options[default_idx].to_string()
}

fn prompt_yn(message: &str, default: &str, reader: &mut dyn BufRead, writer: &mut dyn Write) -> Option<bool> {
    let answer = prompt(&format!("{} (y/n)", message), Some(default), reader, writer);
    if answer.to_lowercase() == "q" {
        return None;
    }
    Some(matches!(answer.to_lowercase().as_str(), "y" | "yes"))
}

fn parse_hole_cards(text: &str) -> Option<Vec<Card>> {
    let text = text.trim().replace(' ', "");
    if text.len() != 4 {
        return None;
    }
    let c1 = parse_card(&text[..2]).ok()?;
    let c2 = parse_card(&text[2..]).ok()?;
    if c1 == c2 {
        return None;
    }
    Some(vec![c1, c2])
}

fn parse_board_input(text: &str) -> Option<Vec<Card>> {
    let text = text.trim().replace(' ', "");
    if text.len() % 2 != 0 {
        return None;
    }
    parse_board(&text).ok()
}

fn parse_sizing_pct(sizing: &str) -> Option<f64> {
    let mut nums = String::new();
    let mut found_digit = false;
    for ch in sizing.chars() {
        if ch.is_ascii_digit() || ch == '-' {
            nums.push(ch);
            found_digit = true;
        } else if found_digit {
            break;
        }
    }
    if nums.is_empty() {
        return None;
    }
    if nums.contains('-') {
        let parts: Vec<&str> = nums.split('-').collect();
        if parts.len() == 2 {
            let a: f64 = parts[0].parse().ok()?;
            let b: f64 = parts[1].parse().ok()?;
            return Some((a + b) / 200.0);
        }
    }
    let n: f64 = nums.parse().ok()?;
    Some(n / 100.0)
}

// ---------------------------------------------------------------------------
// Interactive session
// ---------------------------------------------------------------------------

struct QuitSession;

pub fn play_command() {
    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut reader = stdin.lock();
    let mut writer = stdout.lock();
    run_interactive_session(&mut reader, &mut writer);
}

pub fn run_interactive_session(reader: &mut dyn BufRead, writer: &mut dyn Write) {
    writeln!(writer).ok();
    writeln!(writer, "{}", "GTO Play \u{2014} solver-backed interactive advisor".cyan().bold()).ok();
    writeln!(writer, "Type {} at any prompt to quit. Defaults: 6max, 100bb, heads-up SRP.\n", "'q'".bold()).ok();

    // Initialize strategy engine (100bb default)
    let mut engine = StrategyEngine::new(100.0);
    if engine.has_preflop() {
        writeln!(writer, "  {} Preflop solver loaded", "\u{2713}".green()).ok();
    } else {
        writeln!(writer, "  {} No preflop solution \u{2014} run `gto solve preflop` for solver-backed advice", "\u{2717}".yellow()).ok();
    }

    loop {
        match play_one_hand(&mut engine, reader, writer) {
            Ok(()) => {}
            Err(QuitSession) => {
                writeln!(writer, "\n{}\n", "Good luck at the tables.".cyan().bold()).ok();
                return;
            }
        }

        match prompt_yn("\nPlay another hand?", "y", reader, writer) {
            Some(true) => continue,
            _ => {
                writeln!(writer, "\n{}\n", "Good luck at the tables.".cyan().bold()).ok();
                return;
            }
        }
    }
}

fn play_one_hand(
    engine: &mut StrategyEngine,
    reader: &mut dyn BufRead,
    writer: &mut dyn Write,
) -> Result<(), QuitSession> {
    let table_size = "6max";
    let bb_amount = 1.0; // Use bb as unit
    let valid_positions = positions_for(table_size);

    // -- Hole cards --
    let hole_cards = loop {
        let cards_str = prompt("  Hand", None, reader, writer);
        if cards_str.to_lowercase() == "q" {
            return Err(QuitSession);
        }
        if let Some(cards) = parse_hole_cards(&cards_str) {
            break cards;
        }
        writeln!(writer, "  {}", "Invalid. Use format: AhKs, Td9c, 7s7h".red()).ok();
    };

    let hand_str: String = hole_cards.iter().map(|c| format!("{}", c)).collect();
    let hand_name = simplify_hand(&hole_cards).unwrap_or_else(|_| "??".to_string());
    let pretty: String = hole_cards.iter().map(|c| c.pretty()).collect::<Vec<_>>().join("");
    writeln!(writer, "  {} ({})", pretty.bold(), hand_name.dimmed()).ok();

    // -- Position --
    let default_btn_idx = valid_positions.iter().position(|&p| p == "BTN").unwrap_or(0);
    let pos_str = prompt_menu("Position", valid_positions, default_btn_idx, reader, writer);
    if pos_str.to_lowercase() == "q" {
        return Err(QuitSession);
    }
    let hero_pos = pos_str.to_uppercase();
    let hero_pos = if valid_positions.contains(&hero_pos.as_str()) {
        hero_pos
    } else {
        "BTN".to_string()
    };

    let hero = Position::from_str(&hero_pos).unwrap_or(Position::BTN);
    let villain = default_villain(hero);
    let villain_pos_str = villain.as_str().to_string();

    // -- Preflop advice --
    writeln!(writer, "\n{}", "--- Preflop ---".cyan().bold()).ok();

    let has_solver_preflop = show_preflop_advice(
        engine, &hand_name, &hand_str, &hero_pos, hero, table_size, writer,
    );

    // Check if we should fold
    if should_fold_preflop(engine, &hand_name, hero) {
        writeln!(writer, "\n  {}", "Hand over \u{2014} fold preflop.".dimmed()).ok();
        return Ok(());
    }

    // -- Pot tracking (in bb) --
    let mut pot = 6.0; // SRP default: 2.5bb open + BB call + 1.5bb blinds
    let mut remaining_stack = 97.0; // 100bb - 3bb invested

    let hero_ip = hero.is_ip_vs(&villain);
    let ip_label = if hero_ip { "IP" } else { "OOP" };

    // -- Postflop streets --
    let mut board: Vec<Card> = Vec::new();
    let mut board_str = String::new();

    for &(street_name, num_cards) in &[("Flop", 3usize), ("Turn", 1usize), ("River", 1usize)] {
        // Get street cards
        let new_cards = loop {
            let example = if num_cards == 3 { "Ks7d2c" } else { "Jh" };
            let label = if num_cards > 1 { "cards" } else { "card" };
            let input = prompt(
                &format!("  {} {}", street_name, label),
                None,
                reader,
                writer,
            );
            if input.to_lowercase() == "q" {
                return Err(QuitSession);
            }
            if input.is_empty() {
                // Skip to end
                writeln!(writer, "\n  {}", format!("Hand ended before the {}.", street_name.to_lowercase()).dimmed()).ok();
                return Ok(());
            }
            if let Some(cards) = parse_board_input(&input) {
                if cards.len() != num_cards {
                    writeln!(writer, "  {}", format!("Need {} {}. (e.g. {})", num_cards, label, example).red()).ok();
                    continue;
                }
                let known: HashSet<Card> = hole_cards.iter().chain(board.iter()).copied().collect();
                if cards.iter().any(|c| known.contains(c)) {
                    writeln!(writer, "  {}", "Duplicate card. Try again.".red()).ok();
                    continue;
                }
                break cards;
            }
            writeln!(writer, "  {}", format!("Invalid. (e.g. {})", example).red()).ok();
        };
        board.extend(&new_cards);
        board_str = board.iter().map(|c| format!("{}", c)).collect();

        // Show solver-backed advice or fall back to heuristics
        writeln!(writer, "\n{}", format!("--- {} ---", capitalize(street_name)).cyan().bold()).ok();
        writeln!(writer, "  Board: {}  |  {}  |  Pot: {:.0}bb  |  Stack: {:.0}bb",
            board_display(&board), ip_label, pot, remaining_stack).ok();

        show_street_advice(
            engine,
            &hand_str,
            hero,
            villain,
            &board,
            &board_str,
            pot,
            remaining_stack,
            ip_label,
            street_name.to_lowercase().as_str(),
            &hole_cards,
            &hero_pos,
            &villain_pos_str,
            table_size,
            writer,
        );

        // Ask what happened and update pot
        match update_pot_after_action(pot, remaining_stack, reader, writer) {
            Some((new_pot, new_stack)) => {
                pot = new_pot;
                remaining_stack = new_stack;
            }
            None => return Err(QuitSession),
        }
    }

    writeln!(writer, "\n{}", "--- Hand Complete ---".cyan().bold()).ok();
    Ok(())
}

// ---------------------------------------------------------------------------
// Solver-backed advice
// ---------------------------------------------------------------------------

/// Show preflop advice using solver if available, falling back to heuristics.
/// Returns true if solver was used.
fn show_preflop_advice(
    engine: &StrategyEngine,
    hand_name: &str,
    _hand_str: &str,
    hero_pos: &str,
    hero: Position,
    table_size: &str,
    writer: &mut dyn Write,
) -> bool {
    // Try solver first
    if engine.has_preflop() {
        if let Some(result) = engine.query_preflop(hand_name, hero, None) {
            writeln!(writer, "  {}", format_strategy(&result)).ok();
            return true;
        }
    }

    // Fall back to heuristic
    let pf_action = preflop_action(hand_name, hero_pos, "RFI", None, table_size)
        .unwrap_or_else(|_| crate::preflop::PreflopAction {
            action: "FOLD".to_string(),
            detail: "Unknown hand".to_string(),
            hand: hand_name.to_string(),
            position: hero_pos.to_string(),
        });

    writeln!(writer, "  \u{2192} {}", styled_action(&pf_action.action)).ok();
    if !engine.has_preflop() {
        writeln!(writer, "  {}", "Tip: run `gto solve preflop` for solver-backed advice".dimmed()).ok();
    }
    false
}

/// Check if we should fold preflop.
fn should_fold_preflop(engine: &StrategyEngine, hand_name: &str, hero: Position) -> bool {
    if engine.has_preflop() {
        if let Some(result) = engine.query_preflop(hand_name, hero, None) {
            // Fold if the dominant action is FOLD (>80%)
            if let Some(fold_idx) = result.actions.iter().position(|a| a == "FOLD") {
                return result.frequencies[fold_idx] > 0.80;
            }
        }
    }
    // Fall back: fold if heuristic says fold
    let action = preflop_action(hand_name, hero.as_str(), "RFI", None, "6max");
    matches!(action, Ok(ref a) if a.action == "FOLD")
}

/// Show postflop street advice. Uses solver when available, falls back to heuristics.
fn show_street_advice(
    engine: &mut StrategyEngine,
    hand_str: &str,
    hero: Position,
    villain: Position,
    board: &[Card],
    board_str: &str,
    pot: f64,
    stack: f64,
    ip_label: &str,
    street: &str,
    hole_cards: &[Card],
    hero_pos: &str,
    villain_pos: &str,
    table_size: &str,
    writer: &mut dyn Write,
) {
    // Try solver-backed advice
    let iterations = match street {
        "flop" => 500000,
        "turn" => 5000,
        "river" => 10000,
        _ => 10000,
    };

    match engine.query_postflop(hand_str, hero, villain, board_str, pot, stack, iterations) {
        Ok(result) if result.source != StrategySource::NotInRange && !result.actions.is_empty() => {
            writeln!(writer, "  {}", format_strategy(&result)).ok();
            return;
        }
        Ok(result) if result.source == StrategySource::NotInRange => {
            writeln!(writer, "  {} not in solver range \u{2014} using heuristic", hand_str.dimmed()).ok();
        }
        Err(_) => {
            // Solver failed, fall through to heuristic
        }
        _ => {}
    }

    // Fall back to heuristic analysis
    show_heuristic_analysis(
        street, hole_cards, board, pot, stack, hero_pos,
        Some(villain_pos), ip_label, "RFI", table_size, writer,
    );
}

/// Heuristic-based postflop analysis (fallback when solver unavailable).
fn show_heuristic_analysis(
    street: &str,
    hole_cards: &[Card],
    board: &[Card],
    pot: f64,
    stack: f64,
    hero_pos: &str,
    villain_pos: Option<&str>,
    ip_label: &str,
    situation: &str,
    table_size: &str,
    writer: &mut dyn Write,
) {
    // Board texture
    let texture = match analyze_board(board) {
        Ok(t) => t,
        Err(e) => {
            writeln!(writer, "  {}", format!("Error: {}", e).red()).ok();
            return;
        }
    };

    // Hand evaluation
    let hand_result = match evaluate_hand(hole_cards, board) {
        Ok(r) => r,
        Err(e) => {
            writeln!(writer, "  {}", format!("Error: {}", e).red()).ok();
            return;
        }
    };
    writeln!(writer, "  Made: {}", hand_result.category.to_string().bold()).ok();

    // Equity
    let villain_range = estimate_villain_range(situation, hero_pos, villain_pos, hole_cards, table_size);
    let equity = match equity_vs_range(hole_cards, &villain_range, Some(board), 10000) {
        Ok(result) => {
            let eq = result.equity();
            writeln!(writer, "  Equity: {}", equity_bar(eq, 30)).ok();
            eq
        }
        Err(_) => 0.5,
    };

    let strength = classify_hand_strength(&hand_result, hole_cards, board, equity);

    // Strategy recommendation
    let strat = street_strategy(strength, &texture, pot, stack, ip_label, street);
    writeln!(writer, "  \u{2192} {} {}", styled_action(&strat.action), strat.sizing).ok();
    writeln!(writer, "  {}", strat.reasoning.dimmed()).ok();
}

// ---------------------------------------------------------------------------
// Pot tracking
// ---------------------------------------------------------------------------

fn update_pot_after_action(
    pot: f64,
    stack: f64,
    reader: &mut dyn BufRead,
    writer: &mut dyn Write,
) -> Option<(f64, f64)> {
    let action = prompt_menu(
        "What happened?",
        &["Bet/Raise", "Check", "Call", "Fold", "All-in"],
        0,
        reader,
        writer,
    );
    if action.to_lowercase() == "q" {
        return None;
    }

    let action_lower = action.to_lowercase();
    let action_key = if action_lower.starts_with("bet") || action_lower.starts_with("raise") {
        "bet"
    } else if action_lower.starts_with("check") {
        "check"
    } else if action_lower.starts_with("call") {
        "call"
    } else if action_lower.starts_with("fold") {
        "fold"
    } else if action_lower.starts_with("all") {
        "allin"
    } else {
        "check"
    };

    match action_key {
        "check" => Some((pot, stack)),
        "fold" => Some((pot, stack)),
        "allin" => Some((pot + stack * 2.0, 0.0)),
        "bet" => {
            let default_bet = format!("{}", (pot * 0.5) as u64);
            let amount_str = prompt(&format!("  Bet/raise size (pot={:.0}bb)", pot), Some(&default_bet), reader, writer);
            if amount_str.to_lowercase() == "q" {
                return None;
            }
            let amount: f64 = amount_str.parse().unwrap_or(pot * 0.5);
            Some((pot + amount * 2.0, (stack - amount).max(0.0)))
        }
        "call" => {
            let default_call = format!("{}", (pot * 0.3) as u64);
            let amount_str = prompt(&format!("  Call amount (pot={:.0}bb)", pot), Some(&default_call), reader, writer);
            if amount_str.to_lowercase() == "q" {
                return None;
            }
            let amount: f64 = amount_str.parse().unwrap_or(pot * 0.3);
            Some((pot + amount, (stack - amount).max(0.0)))
        }
        _ => Some((pot, stack)),
    }
}

fn capitalize(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        None => String::new(),
        Some(c) => c.to_uppercase().collect::<String>() + chars.as_str(),
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cards::{Card, Rank, Suit};
    use crate::hand_evaluator::{evaluate_hand, HandCategory};
    use crate::postflop::Wetness;
    use crate::math_engine::SprZone;

    fn card(rank: Rank, suit: Suit) -> Card {
        Card::new(rank, suit)
    }

    // -- Position tests --

    #[test]
    fn test_is_in_position_btn_vs_bb() {
        assert!(is_in_position("BTN", "BB", "6max"));
    }

    #[test]
    fn test_is_in_position_bb_vs_btn() {
        assert!(!is_in_position("BB", "BTN", "6max"));
    }

    #[test]
    fn test_is_in_position_co_vs_utg() {
        assert!(is_in_position("CO", "UTG", "6max"));
    }

    #[test]
    fn test_is_in_position_9max() {
        assert!(is_in_position("BTN", "MP", "9max"));
        assert!(!is_in_position("UTG1", "CO", "9max"));
    }

    #[test]
    fn test_explain_position() {
        assert!(explain_position("BTN").contains("best seat"));
        assert!(explain_position("UTG").contains("first to act"));
        assert!(explain_position("BB").contains("defend wide"));
    }

    // -- Hand strength classifier tests --

    #[test]
    fn test_classify_top_pair_good_kicker() {
        let hole = vec![
            card(Rank::Ace, Suit::Hearts),
            card(Rank::King, Suit::Spades),
        ];
        let board = vec![
            card(Rank::King, Suit::Diamonds),
            card(Rank::Seven, Suit::Clubs),
            card(Rank::Two, Suit::Hearts),
        ];
        let result = evaluate_hand(&hole, &board).unwrap();
        let strength = classify_hand_strength(&result, &hole, &board, 0.72);
        assert_eq!(strength, "strong");
    }

    #[test]
    fn test_classify_set() {
        let hole = vec![
            card(Rank::Seven, Suit::Spades),
            card(Rank::Seven, Suit::Hearts),
        ];
        let board = vec![
            card(Rank::Seven, Suit::Diamonds),
            card(Rank::King, Suit::Clubs),
            card(Rank::Two, Suit::Hearts),
        ];
        let result = evaluate_hand(&hole, &board).unwrap();
        let strength = classify_hand_strength(&result, &hole, &board, 0.85);
        assert_eq!(strength, "very_strong");
    }

    #[test]
    fn test_classify_flush() {
        let hole = vec![
            card(Rank::Ace, Suit::Spades),
            card(Rank::Ten, Suit::Spades),
        ];
        let board = vec![
            card(Rank::King, Suit::Spades),
            card(Rank::Seven, Suit::Spades),
            card(Rank::Two, Suit::Spades),
        ];
        let result = evaluate_hand(&hole, &board).unwrap();
        let strength = classify_hand_strength(&result, &hole, &board, 0.80);
        assert_eq!(strength, "very_strong");
    }

    #[test]
    fn test_classify_weak_high_card() {
        let hole = vec![
            card(Rank::Nine, Suit::Hearts),
            card(Rank::Eight, Suit::Clubs),
        ];
        let board = vec![
            card(Rank::Ace, Suit::Spades),
            card(Rank::King, Suit::Diamonds),
            card(Rank::Two, Suit::Hearts),
        ];
        let result = evaluate_hand(&hole, &board).unwrap();
        let strength = classify_hand_strength(&result, &hole, &board, 0.15);
        assert_eq!(strength, "weak");
    }

    #[test]
    fn test_classify_two_pair() {
        let hole = vec![
            card(Rank::King, Suit::Hearts),
            card(Rank::Seven, Suit::Spades),
        ];
        let board = vec![
            card(Rank::King, Suit::Diamonds),
            card(Rank::Seven, Suit::Clubs),
            card(Rank::Two, Suit::Hearts),
        ];
        let result = evaluate_hand(&hole, &board).unwrap();
        let strength = classify_hand_strength(&result, &hole, &board, 0.80);
        assert_eq!(strength, "strong");
    }

    #[test]
    fn test_classify_nuts_quads() {
        let hole = vec![
            card(Rank::Ace, Suit::Hearts),
            card(Rank::Ace, Suit::Spades),
        ];
        let board = vec![
            card(Rank::Ace, Suit::Diamonds),
            card(Rank::Ace, Suit::Clubs),
            card(Rank::Two, Suit::Hearts),
        ];
        let result = evaluate_hand(&hole, &board).unwrap();
        let strength = classify_hand_strength(&result, &hole, &board, 0.99);
        assert_eq!(strength, "nuts");
    }

    #[test]
    fn test_classify_medium_pair() {
        let hole = vec![
            card(Rank::King, Suit::Hearts),
            card(Rank::Three, Suit::Spades),
        ];
        let board = vec![
            card(Rank::King, Suit::Diamonds),
            card(Rank::Seven, Suit::Clubs),
            card(Rank::Two, Suit::Hearts),
        ];
        let result = evaluate_hand(&hole, &board).unwrap();
        let strength = classify_hand_strength(&result, &hole, &board, 0.60);
        assert_eq!(strength, "medium");
    }

    // -- Villain range estimator tests --

    #[test]
    fn test_estimate_villain_range_rfi() {
        let hero = vec![
            card(Rank::Ace, Suit::Hearts),
            card(Rank::King, Suit::Spades),
        ];
        let range = estimate_villain_range("RFI", "BTN", None, &hero, "6max");
        assert!(!range.is_empty());
        assert!(range.iter().any(|h| h == "QQ"));
    }

    #[test]
    fn test_estimate_villain_range_vs_3bet() {
        let hero = vec![
            card(Rank::Ace, Suit::Hearts),
            card(Rank::Ace, Suit::Spades),
        ];
        let range = estimate_villain_range("vs_3bet", "BTN", Some("CO"), &hero, "6max");
        assert!(!range.is_empty());
        assert!(range.len() < 30);
    }

    // -- Explanation helper tests --

    #[test]
    fn test_explain_hand_category() {
        assert!(explain_hand_category(HandCategory::OnePair).contains("solid"));
        assert!(explain_hand_category(HandCategory::RoyalFlush).contains("best"));
    }

    #[test]
    fn test_explain_board_texture() {
        assert!(explain_board_texture(Wetness::Dry).contains("DRY"));
        assert!(explain_board_texture(Wetness::Wet).contains("WET"));
    }

    #[test]
    fn test_explain_spr() {
        assert!(explain_spr(SprZone::Low).contains("commit"));
        assert!(explain_spr(SprZone::High).contains("implied odds"));
    }

    // -- Parse sizing tests --

    #[test]
    fn test_parse_sizing_pct() {
        assert!((parse_sizing_pct("50% pot").unwrap() - 0.5).abs() < 0.01);
        assert!((parse_sizing_pct("66-75% pot").unwrap() - 0.705).abs() < 0.01);
        assert!(parse_sizing_pct("-").is_none());
    }

    // -- Interactive session test with simulated input --

    #[test]
    fn test_interactive_quit_immediately() {
        let input = b"q\n";
        let mut reader = &input[..];
        let mut output = Vec::new();
        run_interactive_session(&mut reader, &mut output);
        let out = String::from_utf8(output).unwrap();
        assert!(out.contains("GTO Play"));
    }

    #[test]
    fn test_interactive_full_preflop_fold() {
        // UTG, 72o (should fold)
        let input = b"7h2c\nUTG\nn\n";
        let mut reader = &input[..];
        let mut output = Vec::new();
        run_interactive_session(&mut reader, &mut output);
        let out = String::from_utf8(output).unwrap();
        assert!(out.contains("FOLD"));
    }
}
