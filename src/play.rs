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
use crate::ranges::{blockers_remove, range_from_top_pct, HAND_RANKING};

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
            // Set (pocket pair hit board) vs trips
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
            // Simpler check: if both hole cards are the same rank (pocket pair) and one on board
            let is_pocket_pair = hole_cards.len() == 2 && hole_cards[0].rank == hole_cards[1].rank;
            if is_pocket_pair && board.iter().any(|c| c.rank == hole_cards[0].rank) {
                "very_strong"
            } else if is_set {
                "very_strong"
            } else {
                "strong"
            }
        }
        HandCategory::TwoPair => {
            if equity >= 0.55 {
                "strong"
            } else {
                "medium"
            }
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
                // Q+ kicker
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

    // Check for 4+ cards within a 5-card window where at least 1 is hero's
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
        low_vals.push(1); // ace as 1
        if low_vals.len() >= 4 {
            // Check that hero contributes at least one card to this draw
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
    hero_pos: &str,
    villain_pos: Option<&str>,
    hero_cards: &[Card],
    table_size: &str,
) -> Vec<String> {
    let villain_range = match situation {
        "RFI" => {
            // Hero opened, villain called -> roughly top 20%
            range_from_top_pct(20.0).unwrap_or_default()
        }
        "vs_RFI" => {
            // Villain opened from their position
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
            // Villain 3-bet -> premium range ~7%
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
        Ok(0) => "q".to_string(), // EOF
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
    // Extract first number-or-range from "50% pot" or "66-75% pot"
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
    writeln!(writer, "{} I'll walk you through a hand step-by-step.",
             "Welcome to GTO Play!".cyan().bold()).ok();
    writeln!(writer, "Type {} at any prompt to quit.\n", "'q'".bold()).ok();

    // -- Game Setup --
    let table_size_input = prompt("Table size? (6max / 9max)", Some("6max"), reader, writer);
    if table_size_input.to_lowercase() == "q" {
        return;
    }
    let table_size = match table_size_input.to_lowercase().as_str() {
        "9max" => "9max",
        _ => "6max",
    };

    let blinds_str = prompt("Blinds? (e.g. 1/2 or 5/10)", Some("1/2"), reader, writer);
    if blinds_str.to_lowercase() == "q" {
        return;
    }
    let (sb_amount, bb_amount) = parse_blinds(&blinds_str).unwrap_or((1.0, 2.0));

    let default_stack = format!("{}", (bb_amount * 100.0) as u64);
    let stack_str = prompt("Your stack?", Some(&default_stack), reader, writer);
    if stack_str.to_lowercase() == "q" {
        return;
    }
    let hero_stack: f64 = stack_str.parse().unwrap_or(bb_amount * 100.0);

    // -- Hand loop --
    loop {
        match play_one_hand(table_size, sb_amount, bb_amount, hero_stack, reader, writer) {
            Ok(()) => {}
            Err(QuitSession) => {
                writeln!(writer, "\n{}\n", "Thanks for playing! Good luck at the tables.".cyan().bold()).ok();
                return;
            }
        }

        match prompt_yn("\nPlay another hand?", "y", reader, writer) {
            Some(true) => continue,
            _ => {
                writeln!(writer, "\n{}\n", "Thanks for playing! Good luck at the tables.".cyan().bold()).ok();
                return;
            }
        }
    }
}

fn parse_blinds(s: &str) -> Option<(f64, f64)> {
    let cleaned = s.replace(' ', "");
    let parts: Vec<&str> = cleaned.split('/').collect();
    if parts.len() != 2 {
        return None;
    }
    let sb: f64 = parts[0].parse().ok()?;
    let bb: f64 = parts[1].parse().ok()?;
    Some((sb, bb))
}

fn play_one_hand(
    table_size: &str,
    sb_amount: f64,
    bb_amount: f64,
    hero_stack: f64,
    reader: &mut dyn BufRead,
    writer: &mut dyn Write,
) -> Result<(), QuitSession> {
    let valid_positions = positions_for(table_size);
    let positions_display = valid_positions.join(" / ");

    // -- Position --
    let pos_str = prompt(
        &format!("Your position? ({})", positions_display),
        Some("BTN"),
        reader,
        writer,
    );
    if pos_str.to_lowercase() == "q" {
        return Err(QuitSession);
    }
    let hero_pos = pos_str.to_uppercase();
    let hero_pos = if valid_positions.contains(&hero_pos.as_str()) {
        hero_pos
    } else {
        writeln!(writer, "  {}", "Invalid position. Defaulting to BTN".yellow()).ok();
        "BTN".to_string()
    };

    writeln!(writer, "  {}", explain_position(&hero_pos).dimmed()).ok();

    let players_str = prompt("Players in the hand?", Some("2"), reader, writer);
    if players_str.to_lowercase() == "q" {
        return Err(QuitSession);
    }
    let num_players: usize = players_str.parse().unwrap_or(2).max(2);

    // -- Hole cards --
    let hole_cards = loop {
        let cards_str = prompt("Your cards? (e.g. AhKs)", None, reader, writer);
        if cards_str.to_lowercase() == "q" {
            return Err(QuitSession);
        }
        if let Some(cards) = parse_hole_cards(&cards_str) {
            break cards;
        }
        writeln!(writer, "  {}", "Invalid cards. Use format like AhKs, Td9c, 7s7h".red()).ok();
    };

    let hand_name = simplify_hand(&hole_cards).unwrap_or_else(|_| "??".to_string());
    let pretty: String = hole_cards.iter().map(|c| c.pretty()).collect::<Vec<_>>().join(" ");
    writeln!(writer, "\n  Your hand: {}  ({})", pretty.bold(), hand_name).ok();

    // -- Preflop situation --
    let raised = match prompt_yn("Has anyone raised before you?", "n", reader, writer) {
        Some(v) => v,
        None => return Err(QuitSession),
    };

    let mut situation = "RFI";
    let mut villain_pos: Option<String> = None;

    if raised {
        let vp = prompt(
            &format!("Which position raised? ({})", positions_display),
            None,
            reader,
            writer,
        );
        if vp.to_lowercase() == "q" {
            return Err(QuitSession);
        }
        let vp_upper = vp.to_uppercase();
        villain_pos = Some(if valid_positions.contains(&vp_upper.as_str()) {
            vp_upper
        } else {
            writeln!(writer, "  {}", "Defaulting to UTG".yellow()).ok();
            "UTG".to_string()
        });

        situation = if hero_pos == "BB" {
            "bb_defense"
        } else {
            "vs_RFI"
        };

        match prompt_yn("Was there a re-raise (3-bet)?", "n", reader, writer) {
            Some(true) => {
                situation = "vs_3bet";
            }
            None => return Err(QuitSession),
            _ => {}
        }
    }

    // -- Preflop advice --
    writeln!(writer, "\n{}", "--- Preflop ---".cyan().bold()).ok();

    let pf_situation = if situation == "bb_defense" {
        "vs_RFI"
    } else {
        situation
    };
    let pf_action = preflop_action(
        &hand_name,
        &hero_pos,
        pf_situation,
        villain_pos.as_deref(),
        table_size,
    )
    .unwrap_or_else(|_| {
        preflop_action(&hand_name, &hero_pos, "RFI", None, table_size).unwrap()
    });

    writeln!(writer, "\n  Recommendation: {}", styled_action(&pf_action.action)).ok();

    // Plain English explanation
    let rfi_pct = get_rfi_pct(&hero_pos, table_size);
    let hand_top_pct = HAND_RANKING
        .iter()
        .position(|&h| h == hand_name)
        .map(|idx| ((idx + 1) as f64 / HAND_RANKING.len() as f64 * 100.0).round() as u32)
        .unwrap_or(50);

    match situation {
        "RFI" => {
            let within = if pf_action.action == "RAISE" {
                "this is well within range."
            } else {
                "this is outside your opening range."
            };
            writeln!(
                writer,
                "  {}",
                format!(
                    "Why: {} is in the top ~{}% of hands. From {} you open ~{}% \u{2014} {}",
                    hand_name, hand_top_pct, hero_pos, rfi_pct, within
                )
                .dimmed()
            )
            .ok();
        }
        "vs_RFI" | "bb_defense" => {
            let ctx = if situation == "bb_defense" {
                format!(
                    "Why: From the BB vs {} open. {}.",
                    villain_pos.as_deref().unwrap_or("?"),
                    pf_action.detail
                )
            } else {
                format!(
                    "Why: {}. {} is in the top ~{}% of hands.",
                    pf_action.detail, hand_name, hand_top_pct
                )
            };
            writeln!(writer, "  {}", ctx.dimmed()).ok();
        }
        "vs_3bet" => {
            writeln!(
                writer,
                "  {}",
                format!("Why: Facing a 3-bet you need a strong hand. {}.", pf_action.detail).dimmed()
            )
            .ok();
        }
        _ => {}
    }

    writeln!(writer, "  {}", format!("Position: {}", explain_position(&hero_pos)).dimmed()).ok();

    if pf_action.action == "FOLD" {
        writeln!(writer, "\n  {}", "Hand over \u{2014} fold preflop.".dimmed()).ok();
        return Ok(());
    }

    // -- Pot tracking --
    let mut pot = sb_amount + bb_amount;
    match situation {
        "RFI" => pot += bb_amount * 2.5,
        "vs_RFI" | "bb_defense" => pot += bb_amount * 3.0,
        "vs_3bet" => pot += bb_amount * 12.0,
        _ => {}
    }

    let mut remaining_stack = hero_stack - (pot / num_players as f64);
    if remaining_stack < 0.0 {
        remaining_stack = hero_stack * 0.8;
    }

    let hero_ip = if let Some(ref vp) = villain_pos {
        is_in_position(&hero_pos, vp, table_size)
    } else {
        hero_pos == "BTN" || hero_pos == "CO"
    };
    let ip_label = if hero_ip { "IP" } else { "OOP" };

    // -- Postflop streets --
    let mut board: Vec<Card> = Vec::new();

    for &(street_name, num_cards) in &[("Flop", 3usize), ("Turn", 1usize), ("River", 1usize)] {
        match prompt_yn(
            &format!("\nContinue to the {}?", street_name.to_lowercase()),
            "y",
            reader,
            writer,
        ) {
            Some(true) => {}
            Some(false) => {
                writeln!(
                    writer,
                    "\n  {}",
                    format!("Hand ended before the {}.", street_name.to_lowercase()).dimmed()
                )
                .ok();
                return Ok(());
            }
            None => return Err(QuitSession),
        }

        // Get street cards
        let new_cards = loop {
            let example = if num_cards == 3 { "Ks7d2c" } else { "Jh" };
            let label = if num_cards > 1 { "cards" } else { "card" };
            let input = prompt(
                &format!("{} {}? (e.g. {})", street_name, label, example),
                None,
                reader,
                writer,
            );
            if input.to_lowercase() == "q" {
                return Err(QuitSession);
            }
            if let Some(cards) = parse_board_input(&input) {
                if cards.len() != num_cards {
                    writeln!(
                        writer,
                        "  {}",
                        format!("Need exactly {} {}. Use format like {}", num_cards, label, example).red()
                    )
                    .ok();
                    continue;
                }
                // Check duplicates
                let known: HashSet<Card> = hole_cards.iter().chain(board.iter()).copied().collect();
                if cards.iter().any(|c| known.contains(c)) {
                    writeln!(writer, "  {}", "Duplicate card detected. Try again.".red()).ok();
                    continue;
                }
                break cards;
            }
            writeln!(
                writer,
                "  {}",
                format!("Need exactly {} {}. Use format like {}", num_cards, label, example).red()
            )
            .ok();
        };
        board.extend(new_cards);

        show_street_analysis(
            street_name.to_lowercase().as_str(),
            &hole_cards,
            &hand_name,
            &board,
            pot,
            remaining_stack,
            &hero_pos,
            villain_pos.as_deref(),
            ip_label,
            num_players,
            situation,
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

fn show_street_analysis(
    street: &str,
    hole_cards: &[Card],
    _hand_name: &str,
    board: &[Card],
    pot: f64,
    stack: f64,
    hero_pos: &str,
    villain_pos: Option<&str>,
    ip_label: &str,
    num_players: usize,
    situation: &str,
    table_size: &str,
    writer: &mut dyn Write,
) {
    writeln!(writer, "\n{}", format!("--- {} ---", capitalize(street)).cyan().bold()).ok();
    writeln!(writer, "  Board: {}", board_display(board)).ok();

    // Board texture
    let texture = match analyze_board(board) {
        Ok(t) => t,
        Err(e) => {
            writeln!(writer, "  {}", format!("Error analyzing board: {}", e).red()).ok();
            return;
        }
    };
    writeln!(writer, "  Texture: {}", explain_board_texture(texture.wetness)).ok();
    if !texture.draws.is_empty() {
        writeln!(writer, "  Draws: {}", texture.draws.join(", ")).ok();
    }

    // Hand evaluation
    let hand_result = match evaluate_hand(hole_cards, board) {
        Ok(r) => r,
        Err(e) => {
            writeln!(writer, "  {}", format!("Error evaluating hand: {}", e).red()).ok();
            return;
        }
    };
    writeln!(writer, "\n  You made: {}", hand_result.category.to_string().bold()).ok();
    writeln!(writer, "  {}", explain_hand_category(hand_result.category).dimmed()).ok();

    // Equity vs villain range
    let villain_range = estimate_villain_range(situation, hero_pos, villain_pos, hole_cards, table_size);
    let equity = match equity_vs_range(hole_cards, &villain_range, Some(board), 10000) {
        Ok(result) => {
            let eq = result.equity();
            writeln!(writer, "  Equity vs villain: {}", equity_bar(eq, 30)).ok();
            eq
        }
        Err(_) => {
            writeln!(writer, "  Equity vs villain: ~50% (estimated)").ok();
            0.5
        }
    };

    // SPR
    if pot > 0.0 {
        if let Ok(spr_result) = calc_spr(stack, pot) {
            writeln!(writer, "\n  SPR: {}", spr_result).ok();
            writeln!(writer, "  {}", explain_spr(spr_result.zone).dimmed()).ok();
        }
    }

    // Hand strength classification
    let strength = classify_hand_strength(&hand_result, hole_cards, board, equity);
    writeln!(writer, "\n  Strength: {}", strength.bold()).ok();
    writeln!(writer, "  {}", explain_strength(strength).dimmed()).ok();

    // Strategy recommendation
    let strat = street_strategy(strength, &texture, pot, stack, ip_label, street);

    writeln!(writer, "\n  \u{2192} {} {}", styled_action(&strat.action), strat.sizing).ok();
    writeln!(writer, "  {}", format!("Why: {}", strat.reasoning).dimmed()).ok();

    if strat.action.starts_with("BET") && pot > 0.0 {
        if let Some(sizing_pct) = parse_sizing_pct(&strat.sizing) {
            let bet_amount = pot * sizing_pct;
            writeln!(
                writer,
                "  {}",
                format!("Bet amount: ~${:.0} into ${:.0} pot", bet_amount, pot).dimmed()
            )
            .ok();
        }
    }

    // Multiway adjustments
    if num_players > 2 {
        let adj = multiway_range_adjustment(num_players);
        writeln!(
            writer,
            "\n  {}",
            format!("Multiway ({} players): {}", num_players, adj).dimmed()
        )
        .ok();
    }

    // Bluff math on the river
    if street == "river" && (strength == "weak" || strength == "bluff") {
        if pot > 0.0 {
            let bluff_bet = pot * 0.66;
            if let Ok(be) = break_even_pct(pot, bluff_bet) {
                writeln!(
                    writer,
                    "\n  {}",
                    format!(
                        "Bluff math: a 66% pot bet needs villain to fold {:.0}% to break even",
                        be * 100.0
                    )
                    .dimmed()
                )
                .ok();
            }
        }
    }
}

fn update_pot_after_action(
    pot: f64,
    stack: f64,
    reader: &mut dyn BufRead,
    writer: &mut dyn Write,
) -> Option<(f64, f64)> {
    let action = prompt(
        "What happened? (bet/check/call/fold/allin)",
        Some("bet"),
        reader,
        writer,
    );
    if action.to_lowercase() == "q" {
        return None;
    }

    match action.to_lowercase().trim() {
        "check" | "x" => Some((pot, stack)),
        "fold" => Some((pot, stack)),
        "allin" => Some((pot + stack * 2.0, 0.0)),
        "bet" | "raise" => {
            let default_bet = format!("{}", (pot * 0.5) as u64);
            let amount_str = prompt("Bet/raise amount?", Some(&default_bet), reader, writer);
            if amount_str.to_lowercase() == "q" {
                return None;
            }
            let amount: f64 = amount_str.parse().unwrap_or(pot * 0.5);
            Some((pot + amount * 2.0, (stack - amount).max(0.0)))
        }
        "call" => {
            let default_call = format!("{}", (pot * 0.3) as u64);
            let amount_str = prompt("Call amount?", Some(&default_call), reader, writer);
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
        // Top pair with Ace kicker -> "strong"
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
        // Top pair weak kicker -> medium
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
        // Should have removed some hands blocked by hero's AhKs
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
        // 3-bet range should be small (premium)
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
        assert!(out.contains("Welcome to GTO Play!"));
    }

    #[test]
    fn test_interactive_full_preflop_fold() {
        // 6max, 1/2, 200 stack, UTG, 72o (should fold), no raise
        let input = b"6max\n1/2\n200\nUTG\n2\n7h2c\nn\n\nn\n";
        let mut reader = &input[..];
        let mut output = Vec::new();
        run_interactive_session(&mut reader, &mut output);
        let out = String::from_utf8(output).unwrap();
        assert!(out.contains("FOLD"));
        assert!(out.contains("fold preflop"));
    }
}
