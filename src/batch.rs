//! Batch pre-solve: generates a manifest of position × board × pot-type spots
//! and solves them sequentially with resumability (skips existing cache files).

use std::time::Instant;

use colored::Colorize;

use crate::flop_enumerator::generate_canonical_flops;
use crate::flop_solver::{FlopSolverConfig, FlopSolution, solve_flop};
use crate::preflop_solver::{Position, PreflopSolution};
use crate::strategy::{derive_defending_range, derive_opening_range, PotType};

// ---------------------------------------------------------------------------
// Representative flop boards (~50 covering major textures)
// ---------------------------------------------------------------------------

const REPRESENTATIVE_FLOPS: &[&str] = &[
    // High dry
    "As7d2c", "Kh8d3c", "Qd6s2h", "Js7c3d", "Ah9c4d",
    "Kd5s2c", "Qs8d3h", "Jh6c2s",
    // High wet / broadway
    "KsQhTd", "JhTs9c", "QdJsTs", "AhKdQc", "KhJd9c",
    "QdTh8s", "AsTd9c", "KsQd8h",
    // Medium connected
    "9h7d5c", "8s6d4c", "Th8d6c", "9c7h4d", "7s5d3c",
    "8h6s4d", "Ts7d5c", "9s6h3d",
    // Low boards
    "6d4c2s", "5h3d2c", "7c4d2h", "6s3c2d", "5d4h2c",
    "7h3s2d", "6c5d3h", "4s3d2h",
    // Monotone (flush possible)
    "Ks9s4s", "Jh7h3h", "Qd8d2d", "Tc6c3c", "As8s5s",
    "Kh6h2h", "9d5d2d", "Jc4c2c",
    // Paired boards
    "KsKd7c", "9h9d3c", "QcQd5s", "7s7d2c", "AsAd4c",
    "ThTd6s", "5c5d2h", "JsJd3c",
    // Wheel/ace-low
    "5s4d3c", "Ah5d4c", "As3d2c", "4h3d2s",
];

// ---------------------------------------------------------------------------
// Position pairs in priority order
// ---------------------------------------------------------------------------

fn position_pairs() -> Vec<(Position, Position)> {
    use Position::*;
    vec![
        // Most common first
        (BTN, BB),
        (CO, BB),
        (HJ, BB),
        (UTG, BB),
        (SB, BB),
        // Less common but still important
        (BTN, SB),
        (CO, BTN),
        (HJ, BTN),
        (UTG, BTN),
    ]
}

// ---------------------------------------------------------------------------
// Spot manifest
// ---------------------------------------------------------------------------

struct BatchSpot {
    opener: Position,
    responder: Position,
    board: String,
    pot_type: PotType,
    oop_range: String,
    ip_range: String,
    pot: f64,
    stack: f64,
    oop_pos: String,
    ip_pos: String,
}

fn generate_manifest(
    solution: &PreflopSolution,
    stack: f64,
    srp_only: bool,
    all_flops: bool,
) -> Vec<BatchSpot> {
    let pairs = position_pairs();
    let pot_types = if srp_only {
        vec![PotType::Srp]
    } else {
        vec![PotType::Srp, PotType::ThreeBet]
    };

    // Choose boards: all 1,755 canonical flops or 50 representative
    let boards: Vec<String> = if all_flops {
        use crate::flop_enumerator::strategic_priority;
        let mut flops = generate_canonical_flops();
        // Sort by strategic priority descending (A-high first, low boards last)
        flops.sort_by(|a, b| strategic_priority(b).cmp(&strategic_priority(a)));
        flops
    } else {
        REPRESENTATIVE_FLOPS.iter().map(|s| s.to_string()).collect()
    };

    // Pre-compute ranges for each position pair
    let mut pair_data: Vec<(Position, Position, String, String, String, String)> = Vec::new();
    for &(opener, responder) in &pairs {
        let spot = match solution.find_spot(opener, responder) {
            Some(s) => s,
            None => continue,
        };

        let opener_range = derive_opening_range(spot, 0.05);
        let responder_range = derive_defending_range(spot, 0.05);

        if opener_range.is_empty() || responder_range.is_empty() {
            continue;
        }

        let (oop_range, ip_range, oop_pos, ip_pos) = if opener.is_ip_vs(&responder) {
            (responder_range.join(","), opener_range.join(","), responder.as_str().to_string(), opener.as_str().to_string())
        } else {
            (opener_range.join(","), responder_range.join(","), opener.as_str().to_string(), responder.as_str().to_string())
        };

        pair_data.push((opener, responder, oop_range, ip_range, oop_pos, ip_pos));
    }

    let mut spots = Vec::new();

    // Board-first iteration: for each board, solve all position pairs and pot
    // types before moving to the next board. This ensures the highest-priority
    // boards get full position coverage first.
    for board in &boards {
        let board_clean: String = board.chars().filter(|c| !c.is_whitespace()).collect();
        if board_clean.len() != 6 {
            continue;
        }

        for pot_type in &pot_types {
            let (pot, eff_stack) = pot_type.pot_and_stack();
            let scale = stack / 100.0;
            let pot_scaled = pot * scale;
            let stack_scaled = eff_stack * scale;

            for (opener, responder, oop_range, ip_range, oop_pos, ip_pos) in &pair_data {
                spots.push(BatchSpot {
                    opener: *opener,
                    responder: *responder,
                    board: board_clean.clone(),
                    pot_type: *pot_type,
                    oop_range: oop_range.clone(),
                    ip_range: ip_range.clone(),
                    pot: pot_scaled,
                    stack: stack_scaled,
                    oop_pos: oop_pos.clone(),
                    ip_pos: ip_pos.clone(),
                });
            }
        }
    }

    spots
}

// ---------------------------------------------------------------------------
// Batch solver
// ---------------------------------------------------------------------------

pub fn run_batch_solve(stack: f64, srp_only: bool, limit: Option<usize>, iterations: usize, all_flops: bool) {
    // 1. Load preflop solution
    let solution = match PreflopSolution::load("6max", stack, 0.0) {
        Ok(s) => s,
        Err(_) => {
            eprintln!(
                "{}",
                "Error: No preflop solution found. Run `gto solve preflop` first.".red()
            );
            return;
        }
    };

    // 2. Generate manifest
    let mut manifest = generate_manifest(&solution, stack, srp_only, all_flops);

    // Apply limit
    if let Some(max) = limit {
        manifest.truncate(max);
    }

    let total = manifest.len();
    println!();
    println!(
        "  {} Batch solve: {} spots to process",
        "GTO".bold(),
        total.to_string().bold(),
    );
    println!(
        "  Stack: {}bb | Iterations: {} | {} | {} flops",
        stack,
        iterations,
        if srp_only { "SRP only" } else { "SRP + 3-bet pots" },
        if all_flops { "1,755" } else { "50 representative" },
    );
    println!();

    let mut solved = 0;
    let mut skipped = 0;
    let batch_start = Instant::now();

    for (i, spot) in manifest.iter().enumerate() {
        // 3. Check if already cached
        if FlopSolution::load_cache(&spot.board, &spot.oop_pos, &spot.ip_pos, spot.pot, spot.stack).is_some() {
            skipped += 1;
            println!(
                "  [{}/{}] {} {} vs {} ({}) ... {}",
                i + 1,
                total,
                spot.board,
                spot.opener.as_str(),
                spot.responder.as_str(),
                spot.pot_type.as_str(),
                "cached".dimmed(),
            );
            continue;
        }

        // 4. Solve
        print!(
            "  [{}/{}] Solving {} {} vs {} ({}) ... ",
            i + 1,
            total,
            spot.board,
            spot.opener.as_str(),
            spot.responder.as_str(),
            spot.pot_type.as_str(),
        );

        let spot_start = Instant::now();

        let config = match FlopSolverConfig::new(
            &spot.board,
            &spot.oop_range,
            &spot.ip_range,
            spot.pot,
            spot.stack,
            iterations,
        ) {
            Ok(c) => c,
            Err(e) => {
                println!("{}", format!("error: {}", e).red());
                continue;
            }
        };

        let mut result = solve_flop(&config);
        result.oop_pos = spot.oop_pos.clone();
        result.ip_pos = spot.ip_pos.clone();
        result.save_cache();
        solved += 1;

        let elapsed = spot_start.elapsed();
        println!(
            "done ({:.1}s, exploit: {:.4})",
            elapsed.as_secs_f64(),
            result.exploitability,
        );
    }

    let total_elapsed = batch_start.elapsed();
    println!();
    println!(
        "  {} Complete: {} solved, {} cached, {:.1} min total",
        "Batch".bold(),
        solved.to_string().bold(),
        skipped.to_string().bold(),
        total_elapsed.as_secs_f64() / 60.0,
    );
    println!();
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_representative_flops_valid() {
        for &board in REPRESENTATIVE_FLOPS {
            let clean: String = board.chars().filter(|c| !c.is_whitespace()).collect();
            // Should be exactly 6 chars (3 cards × 2 chars each)
            assert!(
                clean.len() == 6,
                "Invalid flop board '{}' (cleaned: '{}', len: {})",
                board,
                clean,
                clean.len()
            );
        }
    }

    #[test]
    fn test_position_pairs_non_empty() {
        let pairs = position_pairs();
        assert!(!pairs.is_empty());
        // BTN vs BB should be first (most common)
        assert_eq!(pairs[0], (Position::BTN, Position::BB));
    }

    #[test]
    fn test_pot_type_scaling() {
        let (pot, stack) = PotType::Srp.pot_and_stack();
        // At 100bb: SRP = 6bb pot, 97bb effective
        assert!((pot - 6.0).abs() < 0.01);
        assert!((stack - 97.0).abs() < 0.01);
    }
}
