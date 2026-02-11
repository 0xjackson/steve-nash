use clap::{Parser, Subcommand, ValueEnum};
use colored::Colorize;
use comfy_table::{Cell, CellAlignment, ContentArrangement, Table};

use crate::cards::parse_board;
use crate::display::{
    board_display, equity_bar, print_error, range_grid, styled_action,
};

const POSITIONS_6MAX: &[&str] = &["UTG", "HJ", "CO", "BTN", "SB", "BB"];
const POSITIONS_9MAX: &[&str] = &["UTG", "UTG1", "UTG2", "MP", "HJ", "CO", "BTN", "SB", "BB"];

#[derive(Parser)]
#[command(name = "gto", version = "1.0.0", about = "GTO Poker Toolkit — preflop ranges, equity, odds, and strategy.")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Clone, ValueEnum)]
enum TableSize {
    #[value(name = "6max")]
    SixMax,
    #[value(name = "9max")]
    NineMax,
}

impl TableSize {
    fn as_str(&self) -> &'static str {
        match self {
            TableSize::SixMax => "6max",
            TableSize::NineMax => "9max",
        }
    }
}

#[derive(Clone, ValueEnum)]
enum Situation {
    #[value(name = "RFI")]
    RFI,
    #[value(name = "vs_RFI")]
    VsRFI,
    #[value(name = "vs_3bet")]
    Vs3Bet,
    #[value(name = "bb_defense")]
    BbDefense,
}

impl Situation {
    fn as_str(&self) -> &'static str {
        match self {
            Situation::RFI => "RFI",
            Situation::VsRFI => "vs_RFI",
            Situation::Vs3Bet => "vs_3bet",
            Situation::BbDefense => "bb_defense",
        }
    }
}

#[derive(Clone, ValueEnum)]
enum ActionSituation {
    #[value(name = "RFI")]
    RFI,
    #[value(name = "vs_RFI")]
    VsRFI,
    #[value(name = "vs_3bet")]
    Vs3Bet,
}

impl ActionSituation {
    fn as_str(&self) -> &'static str {
        match self {
            ActionSituation::RFI => "RFI",
            ActionSituation::VsRFI => "vs_RFI",
            ActionSituation::Vs3Bet => "vs_3bet",
        }
    }
}

#[derive(Clone, ValueEnum)]
enum Street {
    Flop,
    Turn,
    River,
}

impl Street {
    fn as_str(&self) -> &'static str {
        match self {
            Street::Flop => "flop",
            Street::Turn => "turn",
            Street::River => "river",
        }
    }
}

#[derive(Clone, ValueEnum)]
enum Strength {
    Nuts,
    VeryStrong,
    Strong,
    Medium,
    Draw,
    Bluff,
    Weak,
}

impl Strength {
    fn as_str(&self) -> &'static str {
        match self {
            Strength::Nuts => "nuts",
            Strength::VeryStrong => "very_strong",
            Strength::Strong => "strong",
            Strength::Medium => "medium",
            Strength::Draw => "draw",
            Strength::Bluff => "bluff",
            Strength::Weak => "weak",
        }
    }
}

#[derive(Subcommand)]
enum Commands {
    /// Show preflop opening range for a position
    Range {
        /// Position (e.g., UTG, HJ, CO, BTN, SB, BB)
        position: String,
        /// Table format
        #[arg(short = 't', long = "table", default_value = "6max")]
        table_size: TableSize,
        /// Villain position (for vs_RFI / vs_3bet)
        #[arg(long)]
        vs: Option<String>,
        /// Preflop situation
        #[arg(short, long, default_value = "RFI")]
        situation: Situation,
    },
    /// Calculate equity between two hands or hand vs range
    Equity {
        /// Your hand (e.g., AhAs)
        hand1: String,
        /// "vs" keyword (optional)
        versus: Option<String>,
        /// Opponent hand or range (e.g., KsKd or KK)
        hand2: Option<String>,
        /// Board cards (e.g., AsKd5c)
        #[arg(short, long)]
        board: Option<String>,
        /// Number of simulations
        #[arg(short = 'n', long, default_value = "30000")]
        sims: usize,
    },
    /// Calculate pot odds, EV, and implied odds
    Odds {
        /// Current pot size
        pot: f64,
        /// Bet size to call
        bet: f64,
        /// Your equity (0-1) to calculate EV
        #[arg(short, long = "equity")]
        equity_val: Option<f64>,
        /// Expected future winnings for implied odds
        #[arg(short = 'i', long = "implied")]
        future: Option<f64>,
    },
    /// Analyze board texture
    Board {
        /// Board cards (e.g., AsKd7c)
        cards: String,
    },
    /// Full decision advisor — preflop and postflop
    Action {
        /// Your hand (e.g., AKs)
        hand: String,
        /// Your position
        #[arg(short, long)]
        position: String,
        /// Board cards
        #[arg(short, long)]
        board: Option<String>,
        /// Current pot size
        #[arg(long)]
        pot: Option<f64>,
        /// Effective stack size
        #[arg(long)]
        stack: Option<f64>,
        /// Villain position
        #[arg(long)]
        vs: Option<String>,
        /// Preflop situation
        #[arg(short, long, default_value = "RFI")]
        situation: ActionSituation,
        /// Table format
        #[arg(short = 't', long = "table", default_value = "6max")]
        table_size: TableSize,
        /// Number of players in pot
        #[arg(long, default_value = "2")]
        players: usize,
        /// Street (flop, turn, river)
        #[arg(long)]
        street: Option<Street>,
        /// Hand strength category (for postflop)
        #[arg(long)]
        strength: Option<Strength>,
    },
    /// Calculate minimum defense frequency
    Mdf {
        /// Current pot size
        pot: f64,
        /// Bet size
        bet: f64,
        /// Number of players
        #[arg(short = 'n', long, default_value = "2")]
        players: usize,
    },
    /// Analyze stack-to-pot ratio
    Spr {
        /// Effective stack size
        stack_size: f64,
        /// Current pot size
        pot_size: f64,
    },
    /// Count combos in a range
    Combos {
        /// Range expression (e.g., "AA,KK,QQ,AKs" or "TT+")
        range_str: String,
    },
    /// Calculate bluff-to-value ratio and fold equity needed
    Bluff {
        /// Current pot size
        pot: f64,
        /// Bet size
        bet: f64,
    },
    /// Interactive hand advisor — walk through a poker hand step-by-step
    Play,
}

fn validate_position(pos: &str, table_size: &str) -> Result<String, String> {
    let pos = pos.to_uppercase();
    let valid = if table_size == "9max" {
        POSITIONS_9MAX
    } else {
        POSITIONS_6MAX
    };
    if valid.contains(&pos.as_str()) {
        Ok(pos)
    } else {
        Err(format!(
            "Invalid position '{}'. Valid: {}",
            pos,
            valid.join(", ")
        ))
    }
}

pub fn run() {
    let cli = Cli::parse();

    match cli.command {
        Commands::Range {
            position,
            table_size,
            vs,
            situation,
        } => cmd_range(position, table_size.as_str(), vs, situation),
        Commands::Equity {
            hand1,
            versus,
            hand2,
            board,
            sims,
        } => cmd_equity(hand1, versus, hand2, board, sims),
        Commands::Odds {
            pot,
            bet,
            equity_val,
            future,
        } => cmd_odds(pot, bet, equity_val, future),
        Commands::Board { cards } => cmd_board(cards),
        Commands::Action {
            hand,
            position,
            board,
            pot,
            stack,
            vs,
            situation,
            table_size,
            players,
            street,
            strength,
        } => cmd_action(
            hand,
            position,
            board,
            pot,
            stack,
            vs,
            situation,
            table_size.as_str(),
            players,
            street,
            strength,
        ),
        Commands::Mdf { pot, bet, players } => cmd_mdf(pot, bet, players),
        Commands::Spr {
            stack_size,
            pot_size,
        } => cmd_spr(stack_size, pot_size),
        Commands::Combos { range_str } => cmd_combos(range_str),
        Commands::Bluff { pot, bet } => cmd_bluff(pot, bet),
        Commands::Play => crate::play::play_command(),
    }
}

fn cmd_range(position: String, table_size: &str, vs: Option<String>, situation: Situation) {
    use crate::preflop::{
        get_bb_defense, get_rfi_pct, get_rfi_range, get_vs_3bet_range, get_vs_rfi_range,
    };
    use crate::ranges::{range_pct, total_combos};

    let position = match validate_position(&position, table_size) {
        Ok(p) => p,
        Err(e) => {
            print_error(&e);
            return;
        }
    };

    match situation {
        Situation::RFI => {
            let hands = get_rfi_range(&position, table_size);
            let pct = get_rfi_pct(&position, table_size);
            let title = format!("{} RFI Range ({})", position, table_size);

            println!();
            println!("{}", range_grid(&hands, &title));
            println!();
            println!(
                "  {} hands | {} combos | {}% of hands",
                hands.len().to_string().bold(),
                total_combos(&hands).to_string().bold(),
                pct.to_string().bold(),
            );
            println!();
        }
        Situation::VsRFI => {
            let vs = match vs {
                Some(v) => match validate_position(&v, table_size) {
                    Ok(p) => p,
                    Err(e) => {
                        print_error(&e);
                        return;
                    }
                },
                None => {
                    print_error("--vs required for vs_RFI situation");
                    return;
                }
            };
            let result = get_vs_rfi_range(&position, &vs, table_size);

            println!();
            println!("{}", format!("{} vs {} Open ({})", position, vs, table_size).bold());
            println!();

            if !result.three_bet.is_empty() {
                println!(
                    "  {} {}",
                    "3-Bet:".red().bold(),
                    result.three_bet.join(", ")
                );
                println!(
                    "        {} combos ({:.1}%)",
                    total_combos(&result.three_bet),
                    range_pct(&result.three_bet)
                );
            }
            if !result.call.is_empty() {
                println!(
                    "  {} {}",
                    "Call: ".green().bold(),
                    result.call.join(", ")
                );
                println!(
                    "        {} combos ({:.1}%)",
                    total_combos(&result.call),
                    range_pct(&result.call)
                );
            }
            println!("  {}  everything else", "Fold:".dimmed());

            let mut all_hands: Vec<String> = result.three_bet;
            all_hands.extend(result.call);
            println!();
            println!(
                "{}",
                range_grid(&all_hands, &format!("{} vs {}", position, vs))
            );
            println!();
        }
        Situation::Vs3Bet => {
            let vs = match vs {
                Some(v) => match validate_position(&v, table_size) {
                    Ok(p) => p,
                    Err(e) => {
                        print_error(&e);
                        return;
                    }
                },
                None => {
                    print_error("--vs required for vs_3bet situation");
                    return;
                }
            };
            let result = get_vs_3bet_range(&position, &vs, table_size);

            println!();
            println!("{}", format!("{} vs {} 3-Bet ({})", position, vs, table_size).bold());
            println!();

            if !result.four_bet.is_empty() {
                println!(
                    "  {} {}",
                    "4-Bet:".red().bold(),
                    result.four_bet.join(", ")
                );
                println!("        {} combos", total_combos(&result.four_bet));
            }
            if !result.call.is_empty() {
                println!(
                    "  {} {}",
                    "Call: ".green().bold(),
                    result.call.join(", ")
                );
                println!("        {} combos", total_combos(&result.call));
            }
            println!("  {}  everything else", "Fold:".dimmed());
            println!();
        }
        Situation::BbDefense => {
            let vs = match vs {
                Some(v) => match validate_position(&v, table_size) {
                    Ok(p) => p,
                    Err(e) => {
                        print_error(&e);
                        return;
                    }
                },
                None => {
                    print_error("--vs required for bb_defense situation");
                    return;
                }
            };
            let result = get_bb_defense(&vs, table_size);

            println!();
            println!("{}", format!("BB Defense vs {} ({})", vs, table_size).bold());
            println!();

            if !result.three_bet.is_empty() {
                println!(
                    "  {} {}",
                    "3-Bet:".red().bold(),
                    result.three_bet.join(", ")
                );
                println!("        {} combos", total_combos(&result.three_bet));
            }
            if !result.call.is_empty() {
                println!(
                    "  {} {}",
                    "Call: ".green().bold(),
                    result.call.join(", ")
                );
                println!("        {} combos", total_combos(&result.call));
            }
            println!("  {}  everything else", "Fold:".dimmed());

            let mut all_hands: Vec<String> = result.three_bet;
            all_hands.extend(result.call);
            println!();
            println!(
                "{}",
                range_grid(&all_hands, &format!("BB vs {}", vs))
            );
            println!();
        }
    }
}

fn cmd_equity(
    hand1: String,
    versus: Option<String>,
    hand2: Option<String>,
    board: Option<String>,
    sims: usize,
) {
    use crate::cards::parse_card;
    use crate::equity::{equity_vs_hand, equity_vs_range};
    use crate::ranges::parse_range;

    // Handle "gto equity AhAs vs KsKd" or "gto equity AhAs KsKd"
    let hand2 = match (hand2, &versus) {
        (None, Some(v)) if v.to_lowercase() != "vs" => {
            Some(v.clone())
        }
        (h2, _) => h2,
    };

    let hand2 = match hand2 {
        Some(h) => h,
        None => {
            print_error("Usage: gto equity <hand1> vs <hand2|range>");
            return;
        }
    };

    let board_cards = match &board {
        Some(b) => match parse_board(b) {
            Ok(cards) => Some(cards),
            Err(e) => {
                print_error(&e.to_string());
                return;
            }
        },
        None => None,
    };

    let h1: Vec<crate::cards::Card> = {
        let mut cards = Vec::new();
        let chars: Vec<char> = hand1.chars().collect();
        for i in (0..chars.len()).step_by(2) {
            if i + 1 >= chars.len() {
                print_error(&format!("Invalid hand: {}", hand1));
                return;
            }
            let s: String = chars[i..=i + 1].iter().collect();
            match parse_card(&s) {
                Ok(c) => cards.push(c),
                Err(_) => {
                    print_error(&format!("Invalid hand: {}", hand1));
                    return;
                }
            }
        }
        cards
    };

    // Try parsing hand2 as specific cards first
    let is_range = hand2.len() != 4 || {
        let chars: Vec<char> = hand2.chars().collect();
        let mut bad = false;
        for i in (0..chars.len()).step_by(2) {
            if i + 1 >= chars.len() {
                bad = true;
                break;
            }
            let s: String = chars[i..=i + 1].iter().collect();
            if parse_card(&s).is_err() {
                bad = true;
                break;
            }
        }
        bad
    };

    println!();
    let board_str = if let Some(ref bc) = board_cards {
        format!(" on {}", board_display(bc))
    } else {
        String::new()
    };

    if is_range {
        let villain_range = parse_range(&hand2);
        println!(
            "  {} vs {}{}",
            hand1.bold(),
            hand2.bold(),
            board_str
        );
        println!("  Running {} simulations...\n", format!("{}", sims).bold());

        match equity_vs_range(
            &h1,
            &villain_range,
            board_cards.as_deref(),
            sims,
        ) {
            Ok(result) => {
                println!("  Hero:    {}", equity_bar(result.equity(), 30));
                println!("  Villain: {}", equity_bar(1.0 - result.equity(), 30));
                println!();

                let mut table = Table::new();
                table.set_content_arrangement(ContentArrangement::Dynamic);
                table.set_header(vec![Cell::new(""), Cell::new("")]);
                table.add_row(vec![
                    Cell::new("Win".bold().to_string()),
                    Cell::new(format!("{:.1}%", result.win * 100.0)),
                ]);
                table.add_row(vec![
                    Cell::new("Tie".bold().to_string()),
                    Cell::new(format!("{:.1}%", result.tie * 100.0)),
                ]);
                table.add_row(vec![
                    Cell::new("Lose".bold().to_string()),
                    Cell::new(format!("{:.1}%", result.lose * 100.0)),
                ]);
                table.add_row(vec![
                    Cell::new("Equity".bold().to_string()),
                    Cell::new(format!("{:.1}%", result.equity() * 100.0).bold().to_string()),
                ]);
                table.add_row(vec![
                    Cell::new("Sims".bold().to_string()),
                    Cell::new(format!("{}", result.simulations)),
                ]);
                println!("{}", table);
                println!();
            }
            Err(e) => print_error(&e.to_string()),
        }
    } else {
        let h2: Vec<crate::cards::Card> = {
            let chars: Vec<char> = hand2.chars().collect();
            let mut cards = Vec::new();
            for i in (0..chars.len()).step_by(2) {
                let s: String = chars[i..=i + 1].iter().collect();
                cards.push(parse_card(&s).unwrap());
            }
            cards
        };

        println!(
            "  {} vs {}{}",
            hand1.bold(),
            hand2.bold(),
            board_str
        );
        println!("  Running {} simulations...\n", format!("{}", sims).bold());

        match equity_vs_hand(&h1, &h2, board_cards.as_deref(), sims) {
            Ok(result) => {
                println!("  Hero:    {}", equity_bar(result.equity(), 30));
                println!("  Villain: {}", equity_bar(1.0 - result.equity(), 30));
                println!();

                let mut table = Table::new();
                table.set_content_arrangement(ContentArrangement::Dynamic);
                table.set_header(vec![Cell::new(""), Cell::new("")]);
                table.add_row(vec![
                    Cell::new("Win".bold().to_string()),
                    Cell::new(format!("{:.1}%", result.win * 100.0)),
                ]);
                table.add_row(vec![
                    Cell::new("Tie".bold().to_string()),
                    Cell::new(format!("{:.1}%", result.tie * 100.0)),
                ]);
                table.add_row(vec![
                    Cell::new("Lose".bold().to_string()),
                    Cell::new(format!("{:.1}%", result.lose * 100.0)),
                ]);
                table.add_row(vec![
                    Cell::new("Equity".bold().to_string()),
                    Cell::new(format!("{:.1}%", result.equity() * 100.0).bold().to_string()),
                ]);
                table.add_row(vec![
                    Cell::new("Sims".bold().to_string()),
                    Cell::new(format!("{}", result.simulations)),
                ]);
                println!("{}", table);
                println!();
            }
            Err(e) => print_error(&e.to_string()),
        }
    }
}

fn cmd_odds(pot: f64, bet: f64, equity_val: Option<f64>, future: Option<f64>) {
    use crate::math_engine::{ev, implied_odds, pot_odds};

    let needed = match pot_odds(pot, bet) {
        Ok(v) => v,
        Err(e) => {
            print_error(&e.to_string());
            return;
        }
    };

    println!();

    let mut table = Table::new();
    table.set_content_arrangement(ContentArrangement::Dynamic);
    table.set_header(vec![
        Cell::new("Metric".bold().to_string()),
        Cell::new("Value"),
    ]);
    table.add_row(vec![Cell::new("Pot"), Cell::new(format!("${:.0}", pot))]);
    table.add_row(vec![Cell::new("Bet"), Cell::new(format!("${:.0}", bet))]);
    table.add_row(vec![
        Cell::new("Pot Odds"),
        Cell::new(format!("{:.1}%", needed * 100.0)),
    ]);
    table.add_row(vec![
        Cell::new("To Call"),
        Cell::new(format!("${:.0}", bet)),
    ]);
    table.add_row(vec![
        Cell::new("Total Pot"),
        Cell::new(format!("${:.0}", pot + bet + bet)),
    ]);

    if let Some(eq) = equity_val {
        let ev_val = ev(eq, pot, bet);
        let ev_str = if ev_val >= 0.0 {
            format!("${:.2}", ev_val).green().to_string()
        } else {
            format!("${:.2}", ev_val).red().to_string()
        };
        table.add_row(vec![
            Cell::new("Your Equity"),
            Cell::new(format!("{:.1}%", eq * 100.0)),
        ]);
        table.add_row(vec![Cell::new("EV of Call"), Cell::new(ev_str)]);
        let verdict = if ev_val >= 0.0 {
            "CALL".green().bold().to_string()
        } else {
            "FOLD".red().bold().to_string()
        };
        table.add_row(vec![Cell::new("Verdict"), Cell::new(verdict)]);
    }

    if let Some(fut) = future {
        match implied_odds(pot, bet, fut) {
            Ok(imp) => {
                table.add_row(vec![
                    Cell::new("Implied Odds"),
                    Cell::new(format!("{:.1}%", imp * 100.0)),
                ]);
                table.add_row(vec![
                    Cell::new("Future Value"),
                    Cell::new(format!("${:.0}", fut)),
                ]);
            }
            Err(e) => {
                print_error(&e.to_string());
                return;
            }
        }
    }

    println!("{}", table);
    println!();
}

fn cmd_board(cards: String) {
    use crate::postflop::{analyze_board, cbet_recommendation};

    let board_cards = match parse_board(&cards) {
        Ok(c) => c,
        Err(e) => {
            print_error(&e.to_string());
            return;
        }
    };

    let texture = match analyze_board(&board_cards) {
        Ok(t) => t,
        Err(e) => {
            print_error(&e.to_string());
            return;
        }
    };

    println!();
    println!("  Board: {}", board_display(&board_cards));
    println!();

    let mut table = Table::new();
    table.set_content_arrangement(ContentArrangement::Dynamic);
    table.set_header(vec![Cell::new(""), Cell::new("")]);
    table.add_row(vec![
        Cell::new("Texture".bold().to_string()),
        Cell::new(&texture.category),
    ]);
    table.add_row(vec![
        Cell::new("Wetness".bold().to_string()),
        Cell::new(texture.wetness.to_string().to_uppercase()),
    ]);
    table.add_row(vec![
        Cell::new("High Card".bold().to_string()),
        Cell::new(format!("{}", texture.high_card)),
    ]);
    table.add_row(vec![
        Cell::new("Paired".bold().to_string()),
        Cell::new(if texture.is_paired { "Yes" } else { "No" }),
    ]);
    table.add_row(vec![
        Cell::new("Flush Draw".bold().to_string()),
        Cell::new(if texture.flush_draw_possible {
            "Yes"
        } else {
            "No"
        }),
    ]);
    table.add_row(vec![
        Cell::new("Straight Draw".bold().to_string()),
        Cell::new(if texture.straight_draw_possible {
            "Yes"
        } else {
            "No"
        }),
    ]);
    if !texture.draws.is_empty() {
        table.add_row(vec![
            Cell::new("Draws".bold().to_string()),
            Cell::new(texture.draws.join(", ")),
        ]);
    }
    println!("{}", table);
    println!();

    let cbet_ip = cbet_recommendation(&texture, "IP", 5.0, false);
    let cbet_oop = cbet_recommendation(&texture, "OOP", 5.0, false);

    println!("{}", "C-Bet Guidance:".bold());
    println!(
        "  IP:  {:.0}% frequency, {} \u{2014} {}",
        cbet_ip.frequency * 100.0,
        cbet_ip.sizing,
        cbet_ip.reasoning
    );
    println!(
        "  OOP: {:.0}% frequency, {} \u{2014} {}",
        cbet_oop.frequency * 100.0,
        cbet_oop.sizing,
        cbet_oop.reasoning
    );
    println!();
}

fn cmd_action(
    hand: String,
    position: String,
    board: Option<String>,
    pot: Option<f64>,
    stack: Option<f64>,
    vs: Option<String>,
    situation: ActionSituation,
    table_size: &str,
    players: usize,
    street: Option<Street>,
    strength: Option<Strength>,
) {
    use crate::math_engine::spr as calc_spr;
    use crate::multiway::multiway_range_adjustment;
    use crate::postflop::{analyze_board, street_strategy};
    use crate::preflop::preflop_action;

    let position = match validate_position(&position, table_size) {
        Ok(p) => p,
        Err(e) => {
            print_error(&e);
            return;
        }
    };

    println!();
    println!(
        "  {} {}  {} {}  {} {}",
        "Hand:".bold(),
        hand,
        "Position:".bold(),
        position,
        "Table:".bold(),
        table_size
    );

    if board.is_none() {
        let vs_str = vs.as_deref();
        match preflop_action(&hand, &position, situation.as_str(), vs_str, table_size) {
            Ok(result) => {
                println!();
                println!("  Action: {}", styled_action(&result.action));
                println!("  {}", result.detail);
                println!();
            }
            Err(e) => print_error(&e.to_string()),
        }
        return;
    }

    let board_str = board.unwrap();
    let board_cards = match parse_board(&board_str) {
        Ok(c) => c,
        Err(e) => {
            print_error(&e.to_string());
            return;
        }
    };

    let texture = match analyze_board(&board_cards) {
        Ok(t) => t,
        Err(e) => {
            print_error(&e.to_string());
            return;
        }
    };

    println!("  {} {}", "Board:".bold(), board_display(&board_cards));

    if let (Some(p), Some(s)) = (pot, stack) {
        if let Ok(spr_result) = calc_spr(s, p) {
            println!("  {} {}", "SPR:".bold(), spr_result);
            println!("  {}", spr_result.guidance);
        }
    }

    if players > 2 {
        let adj = multiway_range_adjustment(players);
        println!("  {} {}", format!("Multiway ({} players):", players).bold(), adj);
    }

    println!("  {} {}", "Texture:".bold(), texture.category);

    if let (Some(str_enum), Some(st_enum), Some(p), Some(s)) =
        (&strength, &street, pot, stack)
    {
        let pos_type = if position == "BTN" || position == "CO" {
            "IP"
        } else {
            "OOP"
        };
        let strat = street_strategy(str_enum.as_str(), &texture, p, s, pos_type, st_enum.as_str());
        println!();
        println!("  Action: {}  {}", styled_action(&strat.action), strat.sizing);
        println!("  {}", strat.reasoning);
    }

    println!();
}

fn cmd_mdf(pot: f64, bet: f64, players: usize) {
    use crate::math_engine::mdf as calc_mdf;
    use crate::multiway::multiway_defense_freq;

    println!();
    match calc_mdf(bet, pot) {
        Ok(base) => {
            println!("  {} {:.1}%", "MDF:".bold(), base * 100.0);
            println!(
                "  You must defend at least {:.1}% of your range",
                base * 100.0
            );
            println!("  to prevent villain from profiting with any two cards.");

            if players > 2 {
                match multiway_defense_freq(players, bet, pot) {
                    Ok(per_player) => {
                        println!();
                        println!(
                            "  {}",
                            format!("Multiway ({} players):", players).bold()
                        );
                        println!("  Per-player defense: {:.1}%", per_player * 100.0);
                    }
                    Err(e) => print_error(&e.to_string()),
                }
            }
        }
        Err(e) => print_error(&e.to_string()),
    }
    println!();
}

fn cmd_spr(stack_size: f64, pot_size: f64) {
    use crate::math_engine::spr;

    match spr(stack_size, pot_size) {
        Ok(result) => {
            println!();
            let mut table = Table::new();
            table.set_content_arrangement(ContentArrangement::Dynamic);
            table.set_header(vec![Cell::new(""), Cell::new("")]);
            table.add_row(vec![
                Cell::new("Stack".bold().to_string()),
                Cell::new(format!("${:.0}", stack_size)),
            ]);
            table.add_row(vec![
                Cell::new("Pot".bold().to_string()),
                Cell::new(format!("${:.0}", pot_size)),
            ]);
            table.add_row(vec![
                Cell::new("SPR".bold().to_string()),
                Cell::new(format!("{:.1}", result.ratio).bold().to_string()),
            ]);
            table.add_row(vec![
                Cell::new("Zone".bold().to_string()),
                Cell::new(result.zone.to_string().to_uppercase()),
            ]);
            table.add_row(vec![
                Cell::new("Guidance".bold().to_string()),
                Cell::new(result.guidance),
            ]);
            println!("{}", table);
            println!();
        }
        Err(e) => print_error(&e.to_string()),
    }
}

fn cmd_combos(range_str: String) {
    use crate::ranges::{combo_count, parse_range, range_pct, total_combos};

    let hands = parse_range(&range_str);

    println!();
    let mut table = Table::new();
    table.set_content_arrangement(ContentArrangement::Dynamic);
    table.set_header(vec![
        Cell::new("Hand".bold().to_string()),
        Cell::new("Combos").set_alignment(CellAlignment::Right),
    ]);

    for h in &hands {
        table.add_row(vec![
            Cell::new(h.bold().to_string()),
            Cell::new(format!("{}", combo_count(h))),
        ]);
    }

    let total = total_combos(&hands);
    let pct = range_pct(&hands);

    // Add separator and totals
    table.add_row(vec![
        Cell::new("Total".bold().to_string()),
        Cell::new(format!("{}", total).bold().to_string()),
    ]);
    table.add_row(vec![
        Cell::new("% of hands".bold().to_string()),
        Cell::new(format!("{:.1}%", pct).bold().to_string()),
    ]);

    println!("{}", table);
    println!();
    println!("{}", range_grid(&hands, &range_str));
    println!();
}

fn cmd_bluff(pot: f64, bet: f64) {
    use crate::math_engine::{bluff_to_value_ratio, break_even_pct};

    let ratio = match bluff_to_value_ratio(bet, pot) {
        Ok(v) => v,
        Err(e) => {
            print_error(&e.to_string());
            return;
        }
    };
    let be_pct = match break_even_pct(pot, bet) {
        Ok(v) => v,
        Err(e) => {
            print_error(&e.to_string());
            return;
        }
    };

    println!();
    let mut table = Table::new();
    table.set_content_arrangement(ContentArrangement::Dynamic);
    table.set_header(vec![Cell::new(""), Cell::new("")]);
    table.add_row(vec![
        Cell::new("Pot".bold().to_string()),
        Cell::new(format!("${:.0}", pot)),
    ]);
    table.add_row(vec![
        Cell::new("Bet".bold().to_string()),
        Cell::new(format!("${:.0}", bet)),
    ]);
    table.add_row(vec![
        Cell::new("Bluff Ratio".bold().to_string()),
        Cell::new(format!("{:.1}%", ratio * 100.0)),
    ]);
    table.add_row(vec![
        Cell::new("Break-Even".bold().to_string()),
        Cell::new(format!("{:.1}%", be_pct * 100.0)),
    ]);
    println!("{}", table);

    let bluff_times = ratio / (1.0 - ratio);
    println!(
        "\n  For every {} value bet, you can bluff {} times.",
        "1".bold(),
        format!("{:.2}", bluff_times).bold()
    );
    println!(
        "  Villain needs to fold {} for a 0 EV bluff.",
        format!("{:.1}%", be_pct * 100.0).bold()
    );
    println!();
}
