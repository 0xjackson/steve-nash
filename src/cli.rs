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
        /// Use solved ranges instead of static charts
        #[arg(long)]
        solved: bool,
        /// Stack depth for solved ranges (in bb)
        #[arg(long, default_value = "100")]
        stack: f64,
        /// Rake percentage for solved ranges
        #[arg(long, default_value = "0")]
        rake: f64,
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
        /// Your hand (e.g., AKs, QQ, T9o)
        hand: String,
        /// Your position (UTG, HJ, CO, BTN, SB, BB)
        position: String,
        /// Villain position (auto-detects RFI vs 3-bet)
        #[arg(long)]
        vs: Option<String>,
        /// Board cards (e.g., AsKd7c)
        #[arg(short, long)]
        board: Option<String>,
        /// Current pot size
        #[arg(long)]
        pot: Option<f64>,
        /// Effective stack size in bb
        #[arg(long, default_value = "60")]
        stack: f64,
        /// Rake percentage
        #[arg(long, default_value = "0")]
        rake: f64,
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
    /// Query GTO strategy for a hand — `gto query AhKs BTN [Ks9d4c] [--pot 6] [--stack 97]`
    Query {
        /// Your hole cards (e.g., AhKs, QdQc, Td9c)
        hand: String,
        /// Your position (UTG, HJ, CO, BTN, SB, BB)
        position: String,
        /// Villain position override (default: auto-detect)
        #[arg(long)]
        vs: Option<String>,
        /// Board cards — omit for preflop (e.g., Ks9d4c, Ks9d4c7h)
        board: Option<String>,
        /// Pot size in bb (auto-derived from spot if omitted)
        #[arg(long)]
        pot: Option<f64>,
        /// Effective stack in bb
        #[arg(short, long, default_value = "100")]
        stack: f64,
        /// MCCFR iterations for on-demand solving
        #[arg(short, long, default_value = "500000")]
        iterations: usize,
    },
    /// Interactive hand advisor — walk through a poker hand step-by-step
    Play,
    /// Solve GTO strategies using CFR+
    Solve {
        #[command(subcommand)]
        solver: SolverCommands,
    },
}

#[derive(Subcommand)]
enum SolverCommands {
    /// Solve push/fold ranges for a given stack depth
    Pushfold {
        /// Stack depth in big blinds
        #[arg(short, long, default_value = "10")]
        stack: f64,
        /// Rake percentage (0-100)
        #[arg(short, long, default_value = "0")]
        rake: f64,
        /// Number of CFR+ iterations (more = more accurate)
        #[arg(short, long, default_value = "10000")]
        iterations: usize,
    },
    /// Solve full preflop decision tree (open/3-bet/4-bet)
    Preflop {
        /// Table format
        #[arg(short = 't', long = "table", default_value = "6max")]
        table_size: TableSize,
        /// Stack depth in big blinds
        #[arg(short, long, default_value = "100")]
        stack: f64,
        /// Rake percentage (0-100)
        #[arg(short, long, default_value = "0")]
        rake: f64,
        /// Number of CFR+ iterations (more = more accurate)
        #[arg(short, long, default_value = "50000")]
        iterations: usize,
    },
    /// Solve a river spot using CFR+
    River {
        /// Board cards (exactly 5 for river, e.g., Ks9d4c7hQc)
        #[arg(short, long)]
        board: String,
        /// OOP player range (e.g., "AA,AKs,KQs")
        #[arg(long)]
        oop: String,
        /// IP player range (e.g., "QQ,JJ,TT")
        #[arg(long)]
        ip: String,
        /// Starting pot size
        #[arg(short, long, default_value = "10")]
        pot: f64,
        /// Effective stack remaining
        #[arg(short, long, default_value = "20")]
        stack: f64,
        /// Number of CFR+ iterations
        #[arg(short, long, default_value = "10000")]
        iterations: usize,
    },
    /// Solve a turn spot using CFR+ (turn + river)
    Turn {
        /// Board cards (exactly 4 for turn, e.g., Ks9d4c7h)
        #[arg(short, long)]
        board: String,
        /// OOP player range (e.g., "AA,AKs,KQs")
        #[arg(long)]
        oop: String,
        /// IP player range (e.g., "QQ,JJ,TT")
        #[arg(long)]
        ip: String,
        /// Starting pot size
        #[arg(short, long, default_value = "10")]
        pot: f64,
        /// Effective stack remaining
        #[arg(short, long, default_value = "20")]
        stack: f64,
        /// Number of CFR+ iterations
        #[arg(short, long, default_value = "5000")]
        iterations: usize,
    },
    /// Solve a flop spot using MCCFR (flop + turn + river)
    Flop {
        /// Board cards (exactly 3 for flop, e.g., Ks9d4c)
        #[arg(short, long)]
        board: String,
        /// OOP player range (e.g., "AA,AKs,KQs")
        #[arg(long)]
        oop: String,
        /// IP player range (e.g., "QQ,JJ,TT")
        #[arg(long)]
        ip: String,
        /// Starting pot size
        #[arg(short, long, default_value = "10")]
        pot: f64,
        /// Effective stack remaining
        #[arg(short, long, default_value = "50")]
        stack: f64,
        /// Number of MCCFR iterations
        #[arg(short, long, default_value = "500000")]
        iterations: usize,
    },
    /// Batch pre-solve flop spots across positions and boards
    Batch {
        /// Stack depth in big blinds
        #[arg(short, long, default_value = "100")]
        stack: f64,
        /// Only solve single raised pots (skip 3-bet pots)
        #[arg(long)]
        srp_only: bool,
        /// Number of MCCFR iterations per spot
        #[arg(short, long, default_value = "500000")]
        iterations: usize,
        /// Maximum number of spots to solve
        #[arg(long)]
        limit: Option<usize>,
        /// Use all 1,755 canonical flops instead of 50 representative
        #[arg(long)]
        all_flops: bool,
    },
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
    dispatch(cli);
}

pub fn run_with_args(args: Vec<String>) {
    let cli = Cli::parse_from(args);
    dispatch(cli);
}

fn dispatch(cli: Cli) {
    match cli.command {
        Commands::Range {
            position,
            table_size,
            vs,
            situation,
            solved,
            stack,
            rake,
        } => {
            if solved {
                cmd_range_solved(position, table_size.as_str(), vs, situation, stack, rake);
            } else {
                cmd_range(position, table_size.as_str(), vs, situation);
            }
        }
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
            table_size,
            players,
            street,
            strength,
            rake,
        } => {
            if board.is_none() {
                cmd_action_preflop(hand, position, vs, table_size.as_str(), stack, rake);
            } else {
                // Infer situation for postflop static advisor
                let situation = if vs.is_some() {
                    ActionSituation::VsRFI
                } else {
                    ActionSituation::RFI
                };
                cmd_action(
                    hand,
                    position,
                    board,
                    pot,
                    Some(stack),
                    vs,
                    situation,
                    table_size.as_str(),
                    players,
                    street,
                    strength,
                );
            }
        }
        Commands::Mdf { pot, bet, players } => cmd_mdf(pot, bet, players),
        Commands::Spr {
            stack_size,
            pot_size,
        } => cmd_spr(stack_size, pot_size),
        Commands::Combos { range_str } => cmd_combos(range_str),
        Commands::Bluff { pot, bet } => cmd_bluff(pot, bet),
        Commands::Query {
            hand,
            position,
            vs,
            board,
            pot,
            stack,
            iterations,
        } => cmd_query(hand, position, vs, board, pot, stack, iterations),
        Commands::Play => crate::play::play_command(),
        Commands::Solve { solver } => match solver {
            SolverCommands::Pushfold {
                stack,
                rake,
                iterations,
            } => cmd_solve_pushfold(stack, rake, iterations),
            SolverCommands::Preflop {
                table_size,
                stack,
                rake,
                iterations,
            } => cmd_solve_preflop(table_size, stack, rake, iterations),
            SolverCommands::River {
                board,
                oop,
                ip,
                pot,
                stack,
                iterations,
            } => cmd_solve_river(board, oop, ip, pot, stack, iterations),
            SolverCommands::Turn {
                board,
                oop,
                ip,
                pot,
                stack,
                iterations,
            } => cmd_solve_turn(board, oop, ip, pot, stack, iterations),
            SolverCommands::Flop {
                board,
                oop,
                ip,
                pot,
                stack,
                iterations,
            } => cmd_solve_flop(board, oop, ip, pot, stack, iterations),
            SolverCommands::Batch {
                stack,
                srp_only,
                iterations,
                limit,
                all_flops,
            } => crate::batch::run_batch_solve(stack, srp_only, limit, iterations, all_flops),
        },
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

fn cmd_range_solved(
    position: String,
    table_size: &str,
    vs: Option<String>,
    situation: Situation,
    stack_bb: f64,
    rake_pct: f64,
) {
    use crate::display::strategy_grid;
    use crate::preflop_solver::{Position, PreflopSolution};

    let position = match validate_position(&position, table_size) {
        Ok(p) => p,
        Err(e) => {
            print_error(&e);
            return;
        }
    };

    let solution = match PreflopSolution::load(table_size, stack_bb, rake_pct) {
        Ok(s) => s,
        Err(_) => {
            print_error(&format!(
                "No cached solution found for {} {}bb {}% rake. Run 'gto solve preflop --stack {} --rake {}' first.",
                table_size, stack_bb, rake_pct, stack_bb, rake_pct,
            ));
            return;
        }
    };

    let pos = match Position::from_str(&position) {
        Some(p) => p,
        None => {
            print_error(&format!("Invalid position: {}", position));
            return;
        }
    };

    match situation {
        Situation::RFI => {
            // Show opener's open frequency from node 100
            // Find any spot where this position is the opener
            let spot = solution.spots.iter().find(|s| s.opener == pos);
            match spot {
                Some(spot) => {
                    println!();
                    println!(
                        "  {} {} Open Range ({:.1}% of hands) | {}bb | Solved",
                        "GTO".bold(),
                        position,
                        spot.open_pct(),
                        stack_bb,
                    );
                    println!();
                    println!("{}", strategy_grid(&spot.open_strategy, &format!(
                        "{} Open Frequency (%)", position
                    )));
                    println!();
                }
                None => {
                    print_error(&format!("No opening spot found for {}", position));
                }
            }
        }
        Situation::VsRFI | Situation::BbDefense => {
            let vs_str = match vs {
                Some(v) => match validate_position(&v, table_size) {
                    Ok(p) => p,
                    Err(e) => { print_error(&e); return; }
                },
                None => {
                    print_error("--vs required for vs_RFI situation");
                    return;
                }
            };

            let vs_pos = match Position::from_str(&vs_str) {
                Some(p) => p,
                None => { print_error(&format!("Invalid position: {}", vs_str)); return; }
            };

            // Find spot where vs_pos is opener and pos is responder
            let spot = solution.find_spot(vs_pos, pos);
            match spot {
                Some(spot) => {
                    println!();
                    println!(
                        "  {} {} vs {} Open | {}bb | Solved",
                        "GTO".bold(),
                        position,
                        vs_str,
                        stack_bb,
                    );
                    println!();
                    println!("{}", strategy_grid(&spot.vs_open_3bet, &format!(
                        "{} 3-Bet Frequency (%) vs {} Open", position, vs_str
                    )));
                    println!();
                    println!("{}", strategy_grid(&spot.vs_open_call, &format!(
                        "{} Call Frequency (%) vs {} Open", position, vs_str
                    )));
                    println!();
                    println!(
                        "  3-Bet: {:.1}% | Flat: {:.1}%",
                        spot.three_bet_pct(),
                        spot.flat_call_pct(),
                    );
                    println!();
                }
                None => {
                    print_error(&format!("No spot found for {} vs {} open", position, vs_str));
                }
            }
        }
        Situation::Vs3Bet => {
            let vs_str = match vs {
                Some(v) => match validate_position(&v, table_size) {
                    Ok(p) => p,
                    Err(e) => { print_error(&e); return; }
                },
                None => {
                    print_error("--vs required for vs_3bet situation");
                    return;
                }
            };

            let vs_pos = match Position::from_str(&vs_str) {
                Some(p) => p,
                None => { print_error(&format!("Invalid position: {}", vs_str)); return; }
            };

            // Find spot where pos is opener and vs_pos is responder (who 3-bet)
            let spot = solution.find_spot(pos, vs_pos);
            match spot {
                Some(spot) => {
                    println!();
                    println!(
                        "  {} {} vs {} 3-Bet | {}bb | Solved",
                        "GTO".bold(),
                        position,
                        vs_str,
                        stack_bb,
                    );
                    println!();
                    println!("{}", strategy_grid(&spot.vs_3bet_4bet, &format!(
                        "{} 4-Bet Frequency (%) vs {} 3-Bet", position, vs_str
                    )));
                    println!();
                    println!("{}", strategy_grid(&spot.vs_3bet_call, &format!(
                        "{} Call Frequency (%) vs {} 3-Bet", position, vs_str
                    )));
                    println!();
                }
                None => {
                    print_error(&format!("No spot found for {} vs {} 3-bet", position, vs_str));
                }
            }
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

fn cmd_action_preflop(
    hand: String,
    position: String,
    vs: Option<String>,
    table_size: &str,
    stack_bb: f64,
    rake: f64,
) {
    use crate::game_tree::hand_to_bucket;
    use crate::preflop_solver::{Position, PreflopSolution};

    let position = match validate_position(&position, table_size) {
        Ok(p) => p,
        Err(e) => {
            print_error(&e);
            return;
        }
    };

    let bucket = match hand_to_bucket(&hand) {
        Some(b) => b,
        None => {
            print_error(&format!("Invalid hand: '{}'. Use format like AKs, QQ, T9o", hand));
            return;
        }
    };

    let solution = match PreflopSolution::load(table_size, stack_bb, rake) {
        Ok(s) => s,
        Err(_) => {
            print_error(&format!(
                "No cached solution for {}bb. Run 'gto solve preflop --stack {}' first.",
                stack_bb, stack_bb,
            ));
            return;
        }
    };

    let pos = match Position::from_str(&position) {
        Some(p) => p,
        None => {
            print_error(&format!("Invalid position: {}", position));
            return;
        }
    };

    println!();
    println!(
        "  {} {} in {} | {}bb",
        "GTO".bold(),
        hand.bold(),
        position.bold(),
        stack_bb,
    );

    match vs {
        None => {
            // RFI — should I open?
            let spot = solution.spots.iter().find(|s| s.opener == pos);
            match spot {
                Some(spot) => {
                    let open_freq = spot.open_strategy[bucket];
                    let fold_freq = 1.0 - open_freq;

                    println!();
                    print_action_freqs(&[("RAISE 2.5bb", open_freq), ("FOLD", fold_freq)]);

                    // Show vs each responder if we're opening
                    if open_freq > 0.1 {
                        println!();
                        println!("  {}", "If facing 3-bet:".bold());
                        for resp_spot in solution.spots.iter().filter(|s| s.opener == pos) {
                            let fourbet = resp_spot.vs_3bet_4bet[bucket];
                            let call3 = resp_spot.vs_3bet_call[bucket];
                            let fold3 = 1.0 - fourbet - call3;
                            if fourbet > 0.01 || call3 > 0.01 {
                                println!(
                                    "    vs {}: 4-Bet {:.0}% | Call {:.0}% | Fold {:.0}%",
                                    resp_spot.responder.as_str().bold(),
                                    fourbet * 100.0,
                                    call3 * 100.0,
                                    fold3 * 100.0,
                                );
                            }
                        }
                    }
                    println!();
                }
                None => {
                    print_error(&format!("No opening spot found for {}", position));
                }
            }
        }
        Some(vs_str) => {
            let vs_str = match validate_position(&vs_str, table_size) {
                Ok(p) => p,
                Err(e) => { print_error(&e); return; }
            };
            let vs_pos = match Position::from_str(&vs_str) {
                Some(p) => p,
                None => { print_error(&format!("Invalid position: {}", vs_str)); return; }
            };

            // Auto-detect: who opened first?
            // Preflop open order: UTG(0) → HJ(1) → CO(2) → BTN(3) → SB(4) → BB(5)
            let hero_order = preflop_open_order(pos);
            let villain_order = preflop_open_order(vs_pos);

            if hero_order > villain_order {
                // Villain opened first, hero responds → vs open
                let spot = solution.find_spot(vs_pos, pos);
                match spot {
                    Some(spot) => {
                        let threebet = spot.vs_open_3bet[bucket];
                        let call = spot.vs_open_call[bucket];
                        let fold = 1.0 - threebet - call;

                        println!("  vs {} open", vs_str.bold());
                        println!();
                        print_action_freqs(&[("3-BET", threebet), ("CALL", call), ("FOLD", fold)]);

                        if threebet > 0.1 {
                            let allin = spot.vs_4bet_allin[bucket];
                            let call4 = spot.vs_4bet_call[bucket];
                            let fold4 = 1.0 - allin - call4;
                            println!();
                            println!("  {}", "If facing 4-bet:".bold());
                            println!(
                                "    5-Bet All-In {:.0}% | Call {:.0}% | Fold {:.0}%",
                                allin * 100.0, call4 * 100.0, fold4 * 100.0,
                            );
                        }
                        println!();
                    }
                    None => {
                        print_error(&format!("No spot for {} vs {} open", position, vs_str));
                    }
                }
            } else {
                // Hero opened first, villain 3-bet → vs 3-bet
                let spot = solution.find_spot(pos, vs_pos);
                match spot {
                    Some(spot) => {
                        let fourbet = spot.vs_3bet_4bet[bucket];
                        let call3 = spot.vs_3bet_call[bucket];
                        let fold3 = 1.0 - fourbet - call3;

                        println!("  vs {} 3-bet", vs_str.bold());
                        println!();
                        print_action_freqs(&[("4-BET", fourbet), ("CALL", call3), ("FOLD", fold3)]);

                        if fourbet > 0.1 {
                            let call5 = spot.vs_5bet_call[bucket];
                            let fold5 = 1.0 - call5;
                            println!();
                            println!("  {}", "If facing 5-bet all-in:".bold());
                            println!(
                                "    Call {:.0}% | Fold {:.0}%",
                                call5 * 100.0, fold5 * 100.0,
                            );
                        }
                        println!();
                    }
                    None => {
                        print_error(&format!("No spot for {} vs {} 3-bet", position, vs_str));
                    }
                }
            }
        }
    }
}

/// Preflop open order (who RFIs first). Lower = opens first.
fn preflop_open_order(pos: crate::preflop_solver::Position) -> usize {
    use crate::preflop_solver::Position;
    match pos {
        Position::UTG => 0,
        Position::HJ => 1,
        Position::CO => 2,
        Position::BTN => 3,
        Position::SB => 4,
        Position::BB => 5,
    }
}

/// Print action frequencies, sorted by frequency, filtering out <1%.
fn print_action_freqs(actions: &[(&str, f64)]) {
    let mut sorted: Vec<(&str, f64)> = actions.to_vec();
    sorted.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());

    let primary = sorted[0];
    if primary.1 > 0.9 {
        println!("  Action: {}  ({:.0}%)", styled_action(primary.0), primary.1 * 100.0);
    } else {
        let parts: Vec<String> = sorted
            .iter()
            .filter(|(_, f)| *f > 0.01)
            .map(|(name, freq)| format!("{} {:.0}%", styled_action(name), freq * 100.0))
            .collect();
        println!("  Action: {}", parts.join(" | "));
    }
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

fn cmd_query(
    hand: String,
    position: String,
    vs: Option<String>,
    board: Option<String>,
    pot: Option<f64>,
    stack: f64,
    iterations: usize,
) {
    use crate::preflop_solver::Position;
    use crate::strategy::{
        default_villain, detect_street, format_strategy, pretty_board, pretty_hand,
        PotType, StrategyEngine, StrategySource,
    };

    let hero = match Position::from_str(&position) {
        Some(p) => p,
        None => {
            print_error(&format!(
                "Invalid position '{}'. Valid: UTG, HJ, CO, BTN, SB, BB",
                position
            ));
            return;
        }
    };

    let villain = match &vs {
        Some(v) => match Position::from_str(v) {
            Some(p) => p,
            None => {
                print_error(&format!("Invalid villain position: {}", v));
                return;
            }
        },
        None => default_villain(hero),
    };

    let mut engine = StrategyEngine::new(stack);

    let hero_side = if hero.is_ip_vs(&villain) { "IP" } else { "OOP" };
    let villain_str = villain.as_str();

    match &board {
        None => {
            // Preflop query
            if !engine.has_preflop() {
                print_error(&format!(
                    "No preflop solution found. Run `gto solve preflop --stack {}` first.",
                    stack
                ));
                return;
            }

            let vs_pos = if vs.is_some() { Some(villain) } else { None };
            match engine.query_preflop(&hand_to_canonical(&hand), hero, vs_pos) {
                Some(result) => {
                    println!();
                    println!(
                        "  {}  {}  {}{}  |  Preflop",
                        "GTO".bold(),
                        pretty_hand(&hand).bold(),
                        position.bold(),
                        if vs.is_some() {
                            format!(" vs {}", villain_str)
                        } else {
                            String::new()
                        },
                    );
                    println!();
                    println!("  {}", format_strategy(&result));
                    println!();
                }
                None => {
                    print_error("Could not find strategy for this hand/position");
                }
            }
        }
        Some(board_str) => {
            // Postflop query
            let street = detect_street(board_str);

            // Auto-derive pot/stack if not specified
            let (pot_val, stack_val) = match pot {
                Some(p) => (p, stack),
                None => PotType::Srp.pot_and_stack(),
            };

            println!();
            println!(
                "  {}  {}  {} vs {}  |  Board: {}  |  {}  |  {}",
                "GTO".bold(),
                pretty_hand(&hand).bold(),
                position.bold(),
                villain_str,
                pretty_board(board_str),
                street,
                hero_side,
            );

            match engine.query_postflop(
                &hand, hero, villain, board_str, pot_val, stack_val, iterations, &[],
            ) {
                Ok(result) => {
                    if result.source == StrategySource::NotInRange {
                        println!();
                        println!("  {} is not in the {} range for this spot", hand, hero_side);
                    } else {
                        println!();
                        println!("  {}", format_strategy(&result));
                    }
                    println!();
                }
                Err(e) => {
                    println!();
                    print_error(&e);
                }
            }
        }
    }
}

/// Convert specific cards "AhKs" to canonical notation "AKo" for preflop lookup.
fn hand_to_canonical(hand: &str) -> String {
    if hand.len() != 4 {
        return hand.to_string();
    }
    let chars: Vec<char> = hand.chars().collect();
    let r1 = chars[0];
    let s1 = chars[1];
    let r2 = chars[2];
    let s2 = chars[3];

    if r1 == r2 {
        format!("{}{}", r1, r2)
    } else if s1 == s2 {
        // Suited — put higher rank first
        let (h, l) = if rank_value(r1) >= rank_value(r2) {
            (r1, r2)
        } else {
            (r2, r1)
        };
        format!("{}{}s", h, l)
    } else {
        // Offsuit — put higher rank first
        let (h, l) = if rank_value(r1) >= rank_value(r2) {
            (r1, r2)
        } else {
            (r2, r1)
        };
        format!("{}{}o", h, l)
    }
}

fn rank_value(c: char) -> u8 {
    match c {
        '2' => 2, '3' => 3, '4' => 4, '5' => 5, '6' => 6, '7' => 7, '8' => 8,
        '9' => 9, 'T' => 10, 'J' => 11, 'Q' => 12, 'K' => 13, 'A' => 14,
        _ => 0,
    }
}

fn cmd_solve_pushfold(stack: f64, rake: f64, iterations: usize) {
    use crate::game_tree::solve_push_fold;

    if stack <= 0.0 {
        print_error("Stack must be positive");
        return;
    }
    if rake < 0.0 || rake > 100.0 {
        print_error("Rake must be between 0 and 100");
        return;
    }

    println!();
    println!(
        "  Solving push/fold for {}bb stack, {}% rake, {} iterations...",
        stack, rake, iterations
    );

    let result = solve_push_fold(stack, iterations, rake);
    result.display();
}

fn cmd_solve_preflop(table_size: TableSize, stack: f64, rake: f64, iterations: usize) {
    use crate::preflop_solver::solve_preflop_6max;

    if stack <= 0.0 {
        print_error("Stack must be positive");
        return;
    }
    if rake < 0.0 || rake > 100.0 {
        print_error("Rake must be between 0 and 100");
        return;
    }

    match table_size {
        TableSize::NineMax => {
            print_error("Preflop solver currently only supports 6max");
            return;
        }
        _ => {}
    }

    println!();
    println!(
        "  {} Solving preflop for {} | {}bb stack | {}% rake | {} iterations",
        "GTO".bold(),
        table_size.as_str(),
        stack,
        rake,
        iterations,
    );
    println!();

    let solution = solve_preflop_6max(stack, iterations, rake);

    // Display summary table
    println!();
    println!("  {}", "Solution Summary".bold());
    println!();

    let mut table = Table::new();
    table.set_content_arrangement(ContentArrangement::Dynamic);
    table.set_header(vec![
        Cell::new("Spot".bold().to_string()),
        Cell::new("Open %").set_alignment(CellAlignment::Right),
        Cell::new("3-Bet %").set_alignment(CellAlignment::Right),
        Cell::new("Flat %").set_alignment(CellAlignment::Right),
        Cell::new("Exploit").set_alignment(CellAlignment::Right),
    ]);

    for spot in &solution.spots {
        table.add_row(vec![
            Cell::new(format!("{} vs {}", spot.opener, spot.responder)),
            Cell::new(format!("{:.1}", spot.open_pct())),
            Cell::new(format!("{:.1}", spot.three_bet_pct())),
            Cell::new(format!("{:.1}", spot.flat_call_pct())),
            Cell::new(format!("{:.4}", spot.exploitability)),
        ]);
    }

    println!("{}", table);

    // Save to disk
    match solution.save() {
        Ok(()) => {
            println!();
            println!(
                "  Solution saved to {}",
                solution.cache_path().display().to_string().dimmed()
            );
            println!(
                "  Use {} to view solved ranges.",
                "gto range <POS> --solved --stack <BB>".bold()
            );
        }
        Err(e) => {
            print_error(&format!("Failed to save solution: {}", e));
        }
    }
    println!();
}

fn cmd_solve_river(board: String, oop: String, ip: String, pot: f64, stack: f64, iterations: usize) {
    use crate::river_solver::{RiverSolverConfig, solve_river};

    if pot <= 0.0 {
        print_error("Pot must be positive");
        return;
    }
    if stack <= 0.0 {
        print_error("Stack must be positive");
        return;
    }

    let config = match RiverSolverConfig::new(&board, &oop, &ip, pot, stack, iterations) {
        Ok(c) => c,
        Err(ref e) => {
            print_error(e);
            return;
        }
    };

    println!();
    println!(
        "  Solving river: board={}, pot={}, stack={}, {} iterations...",
        board, pot, stack, iterations
    );

    let result = solve_river(&config);
    result.display();
    result.save_cache();
}

fn cmd_solve_turn(board: String, oop: String, ip: String, pot: f64, stack: f64, iterations: usize) {
    use crate::turn_solver::{TurnSolverConfig, solve_turn};

    if pot <= 0.0 {
        print_error("Pot must be positive");
        return;
    }
    if stack <= 0.0 {
        print_error("Stack must be positive");
        return;
    }

    let config = match TurnSolverConfig::new(&board, &oop, &ip, pot, stack, iterations) {
        Ok(c) => c,
        Err(ref e) => {
            print_error(e);
            return;
        }
    };

    println!();
    println!(
        "  Solving turn: board={}, pot={}, stack={}, {} iterations...",
        board, pot, stack, iterations
    );

    let result = solve_turn(&config);
    result.display();
    result.save_cache();
}

fn cmd_solve_flop(board: String, oop: String, ip: String, pot: f64, stack: f64, iterations: usize) {
    use crate::flop_solver::{FlopSolverConfig, solve_flop};

    if pot <= 0.0 {
        print_error("Pot must be positive");
        return;
    }
    if stack <= 0.0 {
        print_error("Stack must be positive");
        return;
    }

    let config = match FlopSolverConfig::new(&board, &oop, &ip, pot, stack, iterations) {
        Ok(c) => c,
        Err(ref e) => {
            print_error(e);
            return;
        }
    };

    println!();
    println!(
        "  Solving flop: board={}, pot={}, stack={}, {} iterations...",
        board, pot, stack, iterations
    );

    let result = solve_flop(&config);
    result.display();
    result.save_cache();
}
