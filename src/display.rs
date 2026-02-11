use colored::Colorize;
use comfy_table::{Cell, CellAlignment, ContentArrangement, Table};

use crate::cards::{Card, Suit};

const RANGE_GRID_RANKS: [char; 13] = ['A', 'K', 'Q', 'J', 'T', '9', '8', '7', '6', '5', '4', '3', '2'];

pub fn range_grid(hands_in_range: &[String], title: &str) -> String {
    let in_range: std::collections::HashSet<&str> =
        hands_in_range.iter().map(|s| s.as_str()).collect();

    let mut table = Table::new();
    table.set_content_arrangement(ContentArrangement::Dynamic);

    // Header row
    let mut header = vec![Cell::new("")];
    for &r in &RANGE_GRID_RANKS {
        header.push(Cell::new(r).set_alignment(CellAlignment::Center));
    }
    table.set_header(header);

    for (i, &r1) in RANGE_GRID_RANKS.iter().enumerate() {
        let mut row = vec![Cell::new(format!("{}", r1).bold().to_string())];
        for (j, &r2) in RANGE_GRID_RANKS.iter().enumerate() {
            let hand = if i == j {
                format!("{}{}", r1, r2)
            } else if i < j {
                format!("{}{}s", r1, r2)
            } else {
                format!("{}{}o", r2, r1)
            };

            let cell = if in_range.contains(hand.as_str()) {
                Cell::new(hand.green().bold().to_string())
            } else {
                Cell::new(hand.dimmed().to_string())
            };
            row.push(cell.set_alignment(CellAlignment::Center));
        }
        table.add_row(row);
    }

    format!("  {}\n{}", title.bold(), table)
}

pub fn range_grid_strs(hands_in_range: &[&str], title: &str) -> String {
    let owned: Vec<String> = hands_in_range.iter().map(|s| s.to_string()).collect();
    range_grid(&owned, title)
}

pub fn equity_bar(equity: f64, width: usize) -> String {
    let filled = (equity * width as f64) as usize;
    let bar: String = "\u{2588}".repeat(filled) + &"\u{2591}".repeat(width - filled);
    let pct = format!("{:.1}%", equity * 100.0);

    if equity >= 0.6 {
        format!("{} {}", bar.green(), pct)
    } else if equity >= 0.4 {
        format!("{} {}", bar.yellow(), pct)
    } else {
        format!("{} {}", bar.red(), pct)
    }
}

pub fn board_display(cards: &[Card]) -> String {
    cards
        .iter()
        .map(|card| {
            let rank = card.rank.to_char();
            let symbol = card.suit.symbol();
            let colored = match card.suit {
                Suit::Spades => format!("{}{}", rank, symbol).white().to_string(),
                Suit::Hearts => format!("{}{}", rank, symbol).red().to_string(),
                Suit::Diamonds => format!("{}{}", rank, symbol).blue().to_string(),
                Suit::Clubs => format!("{}{}", rank, symbol).green().to_string(),
            };
            colored
        })
        .collect::<Vec<_>>()
        .join(" ")
}

pub fn odds_table(pot: f64, bet: f64, equity_needed: f64, ev_value: Option<f64>) -> String {
    let mut table = Table::new();
    table.set_content_arrangement(ContentArrangement::Dynamic);

    table.set_header(vec![
        Cell::new("Metric").set_alignment(CellAlignment::Left),
        Cell::new("Value").set_alignment(CellAlignment::Right),
    ]);

    table.add_row(vec![
        Cell::new("Pot".bold().to_string()),
        Cell::new(format!("${:.0}", pot)),
    ]);
    table.add_row(vec![
        Cell::new("Bet".bold().to_string()),
        Cell::new(format!("${:.0}", bet)),
    ]);
    table.add_row(vec![
        Cell::new("Pot Odds".bold().to_string()),
        Cell::new(format!("{:.1}%", equity_needed * 100.0)),
    ]);
    table.add_row(vec![
        Cell::new("To Call".bold().to_string()),
        Cell::new(format!("${:.0}", bet)),
    ]);
    table.add_row(vec![
        Cell::new("Total Pot".bold().to_string()),
        Cell::new(format!("${:.0}", pot + bet + bet)),
    ]);

    if let Some(ev_val) = ev_value {
        let ev_str = if ev_val >= 0.0 {
            format!("${:.2}", ev_val).green().to_string()
        } else {
            format!("${:.2}", ev_val).red().to_string()
        };
        table.add_row(vec![Cell::new("EV".bold().to_string()), Cell::new(ev_str)]);
    }

    table.to_string()
}

pub fn action_style(action: &str) -> &'static str {
    let upper = action.to_uppercase();
    if matches!(
        upper.as_str(),
        "RAISE" | "3BET" | "4BET" | "BET" | "BET (BLUFF)" | "BET (SEMI-BLUFF)"
    ) {
        "red"
    } else if upper == "CALL" {
        "green"
    } else if upper == "FOLD" {
        "dim"
    } else if upper.contains("CHECK") {
        "yellow"
    } else {
        "bold"
    }
}

pub fn styled_action(action: &str) -> String {
    let style = action_style(action);
    match style {
        "red" => action.red().bold().to_string(),
        "green" => action.green().bold().to_string(),
        "dim" => action.dimmed().bold().to_string(),
        "yellow" => action.yellow().bold().to_string(),
        _ => action.bold().to_string(),
    }
}

pub fn print_action(action: &str, detail: &str) {
    let styled = styled_action(action);
    if detail.is_empty() {
        println!("  {}", styled);
    } else {
        println!("  {}  {}", styled, detail);
    }
}

pub fn print_section(title: &str, content: &str) {
    println!("\n{}", title.cyan().bold());
    println!("  {}", content);
}

pub fn print_error(msg: &str) {
    eprintln!("{} {}", "Error:".red().bold(), msg);
}

pub fn print_success(msg: &str) {
    println!("{}", msg.green().bold());
}
