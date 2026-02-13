//! Flop CFR+ solver using External Sampling MCCFR with template trees.
//!
//! Instead of building a full flop→turn→river game tree (which would require
//! ~500K+ nodes), we build 3 separate single-street trees:
//!
//! 1. **Flop tree** — built with actual pot/stack, ~12 nodes
//! 2. **Turn template** — built with pot=1.0, stack=100.0, ~10 nodes
//! 3. **River template** — built with pot=1.0, stack=100.0, ~12 nodes
//!
//! Templates use unit pot sizing. At runtime, payoffs are scaled by the actual
//! pot at the start of that street: `actual_value = template_value × scale_factor`.
//!
//! Each MCCFR iteration samples one turn card and one river card (external
//! sampling), then traverses all three street trees for each traverser combo.
//! This keeps memory trivial (~1 MB) while converging to Nash equilibrium.
//!
//! Hand combos are grouped into equity buckets (~200 per street) to further
//! reduce the info set space.

use rand::Rng;
use rayon::prelude::*;
use serde::{Deserialize, Serialize};

use crate::bucketing::assign_buckets;
use crate::card_encoding::{index_to_card, remaining_deck};
use crate::cards::parse_board;
use crate::flat_cfr::FlatCfr;
use crate::lookup_eval::evaluate_fast;
use crate::postflop_tree::{
    build_tree, collect_node_metadata, Player, TerminalType, TreeConfig, TreeNode,
};
use crate::ranges::parse_range;
use crate::river_solver::{expand_range_to_combos, Combo};

// ---------------------------------------------------------------------------
// Config & result
// ---------------------------------------------------------------------------

pub struct FlopSolverConfig {
    /// 3-card flop board as u8 indices.
    pub board: Vec<u8>,
    pub oop_range: Vec<String>,
    pub ip_range: Vec<String>,
    pub starting_pot: f64,
    pub effective_stack: f64,
    pub iterations: usize,
    pub num_buckets: usize,
}

impl FlopSolverConfig {
    pub fn new(
        board_str: &str,
        oop_range_str: &str,
        ip_range_str: &str,
        starting_pot: f64,
        effective_stack: f64,
        iterations: usize,
    ) -> Result<Self, String> {
        let board_cards = parse_board(board_str).map_err(|e| e.to_string())?;
        if board_cards.len() != 3 {
            return Err("Flop board must have exactly 3 cards".to_string());
        }
        let board: Vec<u8> = board_cards
            .iter()
            .map(|c| crate::card_encoding::card_to_index(c))
            .collect();
        let oop_range = parse_range(oop_range_str);
        let ip_range = parse_range(ip_range_str);

        if oop_range.is_empty() {
            return Err("OOP range is empty".to_string());
        }
        if ip_range.is_empty() {
            return Err("IP range is empty".to_string());
        }

        Ok(FlopSolverConfig {
            board,
            oop_range,
            ip_range,
            starting_pot,
            effective_stack,
            iterations,
            num_buckets: 200,
        })
    }
}

/// Per-node strategy for the flop solution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlopNodeStrategy {
    pub node_id: u16,
    pub player: String,
    pub actions: Vec<String>,
    /// Average strategy frequencies: [combo_idx][action_idx].
    pub frequencies: Vec<Vec<f64>>,
}

/// Bucket-level strategy from a template tree (turn or river within flop solve).
/// Same shape as FlopNodeStrategy but indexed by bucket instead of combo.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TemplateBucketStrategy {
    pub node_id: u16,
    pub player: String,
    pub actions: Vec<String>,
    /// Average strategy frequencies: [bucket_idx][action_idx].
    pub frequencies: Vec<Vec<f64>>,
}

/// Full solution from the flop solver.
#[derive(Debug, Serialize, Deserialize)]
pub struct FlopSolution {
    pub board: String,
    pub oop_range: Vec<String>,
    pub ip_range: Vec<String>,
    pub starting_pot: f64,
    pub effective_stack: f64,
    pub iterations: usize,
    pub exploitability: f64,
    pub oop_combos: Vec<String>,
    pub ip_combos: Vec<String>,
    /// Strategies for flop-level action nodes only.
    pub strategies: Vec<FlopNodeStrategy>,
    /// OOP position label (e.g. "BB") — used in cache key.
    #[serde(default)]
    pub oop_pos: String,
    /// IP position label (e.g. "BTN") — used in cache key.
    #[serde(default)]
    pub ip_pos: String,
    /// Turn template strategies at bucket level (from flop MCCFR training).
    #[serde(default)]
    pub turn_strategies: Vec<TemplateBucketStrategy>,
    /// River template strategies at bucket level (from flop MCCFR training).
    #[serde(default)]
    pub river_strategies: Vec<TemplateBucketStrategy>,
    /// Number of buckets used for turn/river template strategies.
    #[serde(default)]
    pub num_buckets: usize,
}

// ---------------------------------------------------------------------------
// Solver
// ---------------------------------------------------------------------------

/// Solve a flop spot using External Sampling MCCFR with template trees.
pub fn solve_flop(config: &FlopSolverConfig) -> FlopSolution {
    // 1. Build three separate trees
    let flop_tree_config = TreeConfig {
        bet_sizes: vec![0.33, 0.75],
        raise_sizes: vec![1.0],
        max_raises: 2,
        starting_pot: config.starting_pot,
        effective_stack: config.effective_stack,
        add_allin: true,
    };
    let (flop_tree, _flop_nodes) = build_tree(&flop_tree_config);

    let turn_template_config = TreeConfig {
        bet_sizes: vec![0.66],
        raise_sizes: vec![1.0],
        max_raises: 1,
        starting_pot: 1.0,
        effective_stack: 100.0,
        add_allin: false,
    };
    let (turn_template, _turn_nodes) = build_tree(&turn_template_config);

    let river_template_config = TreeConfig {
        bet_sizes: vec![0.5, 1.0],
        raise_sizes: vec![1.0],
        max_raises: 1,
        starting_pot: 1.0,
        effective_stack: 100.0,
        add_allin: false,
    };
    let (river_template, _river_nodes) = build_tree(&river_template_config);

    // 2. Expand ranges to combos
    let oop_combos = expand_range_to_combos(&config.oop_range, &config.board);
    let ip_combos = expand_range_to_combos(&config.ip_range, &config.board);

    if oop_combos.is_empty() || ip_combos.is_empty() {
        return empty_solution(config);
    }

    // 3. Compute flop buckets
    let oop_combo_pairs: Vec<(u8, u8)> = oop_combos.iter().map(|c| (c.0, c.1)).collect();
    let ip_combo_pairs: Vec<(u8, u8)> = ip_combos.iter().map(|c| (c.0, c.1)).collect();

    let flop_oop_buckets =
        assign_buckets(&oop_combo_pairs, &config.board, config.num_buckets, 500);
    let flop_ip_buckets =
        assign_buckets(&ip_combo_pairs, &config.board, config.num_buckets, 500);

    let num_oop_buckets = (*flop_oop_buckets.iter().max().unwrap_or(&0) + 1) as u16;
    let num_ip_buckets = (*flop_ip_buckets.iter().max().unwrap_or(&0) + 1) as u16;

    // 4. Initialize 6 FlatCfr instances (1 per player × 3 streets)
    let flop_metas = collect_node_metadata(&flop_tree);
    let turn_metas = collect_node_metadata(&turn_template);
    let river_metas = collect_node_metadata(&river_template);

    let flop_oop_nodes: Vec<(u8, u16)> = flop_metas
        .iter()
        .map(|m| {
            let hands = if m.player == Player::OOP {
                num_oop_buckets
            } else {
                0
            };
            (m.num_actions, hands)
        })
        .collect();
    let flop_ip_nodes: Vec<(u8, u16)> = flop_metas
        .iter()
        .map(|m| {
            let hands = if m.player == Player::IP {
                num_ip_buckets
            } else {
                0
            };
            (m.num_actions, hands)
        })
        .collect();

    // Turn and river templates use their own bucket counts (recomputed per sampled card)
    // We use the same num_buckets cap for turn/river
    let turn_oop_nodes: Vec<(u8, u16)> = turn_metas
        .iter()
        .map(|m| {
            let hands = if m.player == Player::OOP {
                config.num_buckets as u16
            } else {
                0
            };
            (m.num_actions, hands)
        })
        .collect();
    let turn_ip_nodes: Vec<(u8, u16)> = turn_metas
        .iter()
        .map(|m| {
            let hands = if m.player == Player::IP {
                config.num_buckets as u16
            } else {
                0
            };
            (m.num_actions, hands)
        })
        .collect();

    let river_oop_nodes: Vec<(u8, u16)> = river_metas
        .iter()
        .map(|m| {
            let hands = if m.player == Player::OOP {
                config.num_buckets as u16
            } else {
                0
            };
            (m.num_actions, hands)
        })
        .collect();
    let river_ip_nodes: Vec<(u8, u16)> = river_metas
        .iter()
        .map(|m| {
            let hands = if m.player == Player::IP {
                config.num_buckets as u16
            } else {
                0
            };
            (m.num_actions, hands)
        })
        .collect();

    let mut flop_oop_cfr = FlatCfr::new(&flop_oop_nodes);
    let mut flop_ip_cfr = FlatCfr::new(&flop_ip_nodes);
    let mut turn_oop_cfr = FlatCfr::new(&turn_oop_nodes);
    let mut turn_ip_cfr = FlatCfr::new(&turn_ip_nodes);
    let mut river_oop_cfr = FlatCfr::new(&river_oop_nodes);
    let mut river_ip_cfr = FlatCfr::new(&river_ip_nodes);

    // 5. Precompute blocker sets
    let oop_blockers: Vec<[bool; 52]> = oop_combos
        .iter()
        .map(|c| {
            let mut b = [false; 52];
            b[c.0 as usize] = true;
            b[c.1 as usize] = true;
            b
        })
        .collect();
    let ip_blockers: Vec<[bool; 52]> = ip_combos
        .iter()
        .map(|c| {
            let mut b = [false; 52];
            b[c.0 as usize] = true;
            b[c.1 as usize] = true;
            b
        })
        .collect();

    // Precompute validity tables (which combos don't share cards)
    let valid_ip_for_oop: Vec<Vec<u16>> = oop_combos
        .iter()
        .map(|oop| {
            ip_combos
                .iter()
                .enumerate()
                .filter(|(_, ip)| {
                    oop.0 != ip.0 && oop.0 != ip.1 && oop.1 != ip.0 && oop.1 != ip.1
                })
                .map(|(j, _)| j as u16)
                .collect()
        })
        .collect();
    let valid_oop_for_ip: Vec<Vec<u16>> = ip_combos
        .iter()
        .map(|ip| {
            oop_combos
                .iter()
                .enumerate()
                .filter(|(_, oop)| {
                    ip.0 != oop.0 && ip.0 != oop.1 && ip.1 != oop.0 && ip.1 != oop.1
                })
                .map(|(i, _)| i as u16)
                .collect()
        })
        .collect();

    // Reusable buffers
    let all_metas = [&flop_metas, &turn_metas, &river_metas];
    let max_actions = all_metas
        .iter()
        .flat_map(|ms| ms.iter())
        .map(|m| m.num_actions)
        .max()
        .unwrap_or(1) as usize;
    let mut strategy_buf = vec![0.0f32; max_actions];
    let mut action_values = vec![0.0f32; max_actions];

    // Available turn and river cards
    let remaining_after_flop = remaining_deck(&config.board);
    let num_remaining = remaining_after_flop.len();

    // Build card -> index mapping for remaining cards
    let mut card_to_remaining_idx = [0usize; 52];
    for (idx, &card) in remaining_after_flop.iter().enumerate() {
        card_to_remaining_idx[card as usize] = idx;
    }

    // 6. Precompute bucket and score lookup tables for all runouts
    // This eliminates per-iteration assign_buckets() calls (~100x speedup)

    // Precompute turn buckets: turn_bucket_table[turn_idx] = (oop_buckets, ip_buckets)
    let turn_bucket_table: Vec<(Vec<u16>, Vec<u16>)> = remaining_after_flop
        .par_iter()
        .map(|&turn_card| {
            let turn_board = [config.board[0], config.board[1], config.board[2], turn_card];
            let turn_oop = assign_buckets(&oop_combo_pairs, &turn_board, config.num_buckets, 200);
            let turn_ip = assign_buckets(&ip_combo_pairs, &turn_board, config.num_buckets, 200);
            (turn_oop, turn_ip)
        })
        .collect();

    // Precompute river data for all runouts: river_bucket_table and score_table
    // Indexed by runout_idx = turn_idx * (num_remaining - 1) + adjusted_river_idx
    let num_runouts = num_remaining * (num_remaining - 1);
    let river_bucket_table: Vec<(Vec<u16>, Vec<u16>)>;
    let score_table: Vec<(Vec<u32>, Vec<u32>)>;

    {
        let results: Vec<((Vec<u16>, Vec<u16>), (Vec<u32>, Vec<u32>))> = (0..num_runouts)
            .into_par_iter()
            .map(|runout_idx| {
                let turn_idx = runout_idx / (num_remaining - 1);
                let river_adj = runout_idx % (num_remaining - 1);
                let river_idx = if river_adj >= turn_idx {
                    river_adj + 1
                } else {
                    river_adj
                };
                let turn_card = remaining_after_flop[turn_idx];
                let river_card = remaining_after_flop[river_idx];
                let river_board = [
                    config.board[0],
                    config.board[1],
                    config.board[2],
                    turn_card,
                    river_card,
                ];
                let r_oop =
                    assign_buckets(&oop_combo_pairs, &river_board, config.num_buckets, 0);
                let r_ip =
                    assign_buckets(&ip_combo_pairs, &river_board, config.num_buckets, 0);
                let s_oop: Vec<u32> = oop_combo_pairs
                    .iter()
                    .map(|&(c0, c1)| {
                        evaluate_fast(&[
                            c0,
                            c1,
                            river_board[0],
                            river_board[1],
                            river_board[2],
                            river_board[3],
                            river_board[4],
                        ])
                    })
                    .collect();
                let s_ip: Vec<u32> = ip_combo_pairs
                    .iter()
                    .map(|&(c0, c1)| {
                        evaluate_fast(&[
                            c0,
                            c1,
                            river_board[0],
                            river_board[1],
                            river_board[2],
                            river_board[3],
                            river_board[4],
                        ])
                    })
                    .collect();
                ((r_oop, r_ip), (s_oop, s_ip))
            })
            .collect();

        let (rb, st): (Vec<_>, Vec<_>) = results.into_iter().unzip();
        river_bucket_table = rb;
        score_table = st;
    }

    let mut rng = rand::thread_rng();

    // 7. Run MCCFR iterations
    for iter in 0..config.iterations {
        let traverser = if iter % 2 == 0 {
            Player::OOP
        } else {
            Player::IP
        };

        // Sample a turn card
        let turn_raw_idx = rng.gen_range(0..num_remaining);
        let turn_card = remaining_after_flop[turn_raw_idx];

        // Sample a river card (not the turn card)
        let river_raw_idx = {
            let mut ri;
            loop {
                ri = rng.gen_range(0..num_remaining);
                if ri != turn_raw_idx {
                    break;
                }
            }
            ri
        };
        let river_card = remaining_after_flop[river_raw_idx];

        // Lookup precomputed buckets and scores
        let (turn_oop_buckets, turn_ip_buckets) = &turn_bucket_table[turn_raw_idx];
        let runout_idx = turn_raw_idx * (num_remaining - 1)
            + if river_raw_idx > turn_raw_idx {
                river_raw_idx - 1
            } else {
                river_raw_idx
            };
        let (river_oop_buckets, river_ip_buckets) = &river_bucket_table[runout_idx];
        let (oop_scores, ip_scores) = &score_table[runout_idx];

        let num_combos = match traverser {
            Player::OOP => oop_combos.len(),
            Player::IP => ip_combos.len(),
        };

        // Sequential path for small ranges (< 20 combos)
        if num_combos < 20 {
            for h in 0..num_combos {
                let blocked = match traverser {
                    Player::OOP => {
                        oop_blockers[h][turn_card as usize]
                            || oop_blockers[h][river_card as usize]
                    }
                    Player::IP => {
                        ip_blockers[h][turn_card as usize]
                            || ip_blockers[h][river_card as usize]
                    }
                };
                if blocked { continue; }

                let opp_reach = match traverser {
                    Player::OOP => {
                        let valid = &valid_ip_for_oop[h];
                        let mut reach = vec![0.0f64; ip_combos.len()];
                        for &j in valid {
                            let j = j as usize;
                            if !ip_blockers[j][turn_card as usize]
                                && !ip_blockers[j][river_card as usize]
                            {
                                reach[j] = 1.0;
                            }
                        }
                        reach
                    }
                    Player::IP => {
                        let valid = &valid_oop_for_ip[h];
                        let mut reach = vec![0.0f64; oop_combos.len()];
                        for &i in valid {
                            let i = i as usize;
                            if !oop_blockers[i][turn_card as usize]
                                && !oop_blockers[i][river_card as usize]
                            {
                                reach[i] = 1.0;
                            }
                        }
                        reach
                    }
                };

                let flop_bucket = match traverser {
                    Player::OOP => flop_oop_buckets[h] as usize,
                    Player::IP => flop_ip_buckets[h] as usize,
                };
                let turn_bucket = match traverser {
                    Player::OOP => turn_oop_buckets[h] as usize,
                    Player::IP => turn_ip_buckets[h] as usize,
                };
                let river_bucket = match traverser {
                    Player::OOP => river_oop_buckets[h] as usize,
                    Player::IP => river_ip_buckets[h] as usize,
                };

                cfr_traverse_flop(
                    &flop_tree, traverser, h, flop_bucket, turn_bucket, river_bucket,
                    &opp_reach, &oop_combos, &ip_combos,
                    &oop_blockers, &ip_blockers,
                    &flop_oop_buckets, &flop_ip_buckets,
                    turn_oop_buckets, turn_ip_buckets,
                    river_oop_buckets, river_ip_buckets,
                    oop_scores, ip_scores,
                    &valid_ip_for_oop, &valid_oop_for_ip,
                    config.starting_pot, &turn_template, &river_template,
                    &mut flop_oop_cfr, &mut flop_ip_cfr,
                    &mut turn_oop_cfr, &mut turn_ip_cfr,
                    &mut river_oop_cfr, &mut river_ip_cfr,
                    &mut strategy_buf, &mut action_values,
                    iter,
                );
            }
            continue;
        }

        // Parallel path for large ranges (>= 20 combos)
        // Snapshot CFR instances for parallel readonly traversal
        let snap_flop_oop = flop_oop_cfr.clone();
        let snap_flop_ip = flop_ip_cfr.clone();
        let snap_turn_oop = turn_oop_cfr.clone();
        let snap_turn_ip = turn_ip_cfr.clone();
        let snap_river_oop = river_oop_cfr.clone();
        let snap_river_ip = river_ip_cfr.clone();

        let all_updates: Vec<Vec<RegretUpdate>> = (0..num_combos)
            .into_par_iter()
            .filter_map(|h| {
                let blocked = match traverser {
                    Player::OOP => {
                        oop_blockers[h][turn_card as usize]
                            || oop_blockers[h][river_card as usize]
                    }
                    Player::IP => {
                        ip_blockers[h][turn_card as usize]
                            || ip_blockers[h][river_card as usize]
                    }
                };
                if blocked { return None; }

                let opp_reach = match traverser {
                    Player::OOP => {
                        let valid = &valid_ip_for_oop[h];
                        let mut reach = vec![0.0f64; ip_combos.len()];
                        for &j in valid {
                            let j = j as usize;
                            if !ip_blockers[j][turn_card as usize]
                                && !ip_blockers[j][river_card as usize]
                            {
                                reach[j] = 1.0;
                            }
                        }
                        reach
                    }
                    Player::IP => {
                        let valid = &valid_oop_for_ip[h];
                        let mut reach = vec![0.0f64; oop_combos.len()];
                        for &i in valid {
                            let i = i as usize;
                            if !oop_blockers[i][turn_card as usize]
                                && !oop_blockers[i][river_card as usize]
                            {
                                reach[i] = 1.0;
                            }
                        }
                        reach
                    }
                };

                let flop_bucket = match traverser {
                    Player::OOP => flop_oop_buckets[h] as usize,
                    Player::IP => flop_ip_buckets[h] as usize,
                };
                let turn_bucket = match traverser {
                    Player::OOP => turn_oop_buckets[h] as usize,
                    Player::IP => turn_ip_buckets[h] as usize,
                };
                let river_bucket = match traverser {
                    Player::OOP => river_oop_buckets[h] as usize,
                    Player::IP => river_ip_buckets[h] as usize,
                };

                let mut updates = Vec::new();
                cfr_traverse_flop_ro(
                    &flop_tree, traverser, h, flop_bucket, turn_bucket, river_bucket,
                    &opp_reach, &oop_combos, &ip_combos,
                    &oop_blockers, &ip_blockers,
                    &flop_oop_buckets, &flop_ip_buckets,
                    turn_oop_buckets, turn_ip_buckets,
                    river_oop_buckets, river_ip_buckets,
                    oop_scores, ip_scores,
                    &valid_ip_for_oop, &valid_oop_for_ip,
                    config.starting_pot, &turn_template, &river_template,
                    &snap_flop_oop, &snap_flop_ip,
                    &snap_turn_oop, &snap_turn_ip,
                    &snap_river_oop, &snap_river_ip,
                    &mut updates, iter,
                );
                Some(updates)
            })
            .collect();

        for hand_updates in all_updates {
            for upd in hand_updates {
                let cfr = match (traverser, upd.street) {
                    (Player::OOP, 0) => &mut flop_oop_cfr,
                    (Player::IP, 0) => &mut flop_ip_cfr,
                    (Player::OOP, 1) => &mut turn_oop_cfr,
                    (Player::IP, 1) => &mut turn_ip_cfr,
                    (Player::OOP, 2) => &mut river_oop_cfr,
                    (Player::IP, 2) => &mut river_ip_cfr,
                    _ => unreachable!(),
                };
                cfr.update(upd.node_id, upd.bucket, &upd.action_values, upd.node_value, upd.reach_prob);
            }
        }
    }

    // 7. Extract solution
    extract_solution(
        config,
        &flop_tree,
        &flop_oop_cfr,
        &flop_ip_cfr,
        &oop_combos,
        &ip_combos,
        &flop_oop_buckets,
        &flop_ip_buckets,
        &flop_metas,
        &turn_template,
        &river_template,
        &turn_oop_cfr,
        &turn_ip_cfr,
        &river_oop_cfr,
        &river_ip_cfr,
        &oop_blockers,
        &ip_blockers,
        &valid_ip_for_oop,
        &valid_oop_for_ip,
    )
}

// ---------------------------------------------------------------------------
// MCCFR traversal: flop level
// ---------------------------------------------------------------------------

/// Traverse the flop action tree. At Showdown terminals, chain to the turn template.
#[allow(clippy::too_many_arguments)]
fn cfr_traverse_flop(
    node: &TreeNode,
    traverser: Player,
    hand_idx: usize,
    flop_bucket: usize,
    turn_bucket: usize,
    river_bucket: usize,
    opp_reach: &[f64],
    oop_combos: &[Combo],
    ip_combos: &[Combo],
    oop_blockers: &[[bool; 52]],
    ip_blockers: &[[bool; 52]],
    flop_oop_buckets: &[u16],
    flop_ip_buckets: &[u16],
    turn_oop_buckets: &[u16],
    turn_ip_buckets: &[u16],
    river_oop_buckets: &[u16],
    river_ip_buckets: &[u16],
    oop_scores: &[u32],
    ip_scores: &[u32],
    valid_ip_for_oop: &[Vec<u16>],
    valid_oop_for_ip: &[Vec<u16>],
    flop_pot: f64,
    turn_template: &TreeNode,
    river_template: &TreeNode,
    flop_oop_cfr: &mut FlatCfr,
    flop_ip_cfr: &mut FlatCfr,
    turn_oop_cfr: &mut FlatCfr,
    turn_ip_cfr: &mut FlatCfr,
    river_oop_cfr: &mut FlatCfr,
    river_ip_cfr: &mut FlatCfr,
    strategy_buf: &mut [f32],
    action_values_buf: &mut [f32],
    iter: usize,
) -> f64 {
    match node {
        TreeNode::Terminal {
            terminal_type,
            pot,
            invested,
            ..
        } => {
            let opp_reach_sum: f64 = opp_reach.iter().sum();
            if opp_reach_sum < 1e-10 {
                return 0.0;
            }
            let my_invested = invested[traverser.index()];

            match terminal_type {
                TerminalType::Fold { folder } => {
                    if *folder == traverser {
                        -my_invested * opp_reach_sum
                    } else {
                        (*pot - my_invested) * opp_reach_sum
                    }
                }
                TerminalType::Showdown => {
                    // Chain to turn template, scaling by the pot at this point
                    let turn_scale = *pot;
                    let turn_value = cfr_traverse_turn_template(
                        turn_template,
                        traverser,
                        hand_idx,
                        turn_bucket,
                        river_bucket,
                        opp_reach,
                        oop_combos,
                        ip_combos,
                        turn_oop_buckets,
                        turn_ip_buckets,
                        river_oop_buckets,
                        river_ip_buckets,
                        oop_scores,
                        ip_scores,
                        valid_ip_for_oop,
                        valid_oop_for_ip,
                        turn_scale,
                        river_template,
                        turn_oop_cfr,
                        turn_ip_cfr,
                        river_oop_cfr,
                        river_ip_cfr,
                        strategy_buf,
                        action_values_buf,
                        iter,
                    );
                    // The turn template returns values in template units,
                    // already scaled by turn_scale inside the traversal.
                    // But we also need to account for what was already invested on the flop.
                    turn_value - my_invested * opp_reach_sum
                }
            }
        }
        TreeNode::Action {
            node_id,
            player,
            children,
            actions,
            ..
        } => {
            let num_actions = actions.len();
            let nid = *node_id as usize;

            if *player == traverser {
                let cfr = match traverser {
                    Player::OOP => &*flop_oop_cfr,
                    Player::IP => &*flop_ip_cfr,
                };
                cfr.current_strategy(nid, flop_bucket, strategy_buf);

                let mut node_value = 0.0f64;
                for a in 0..num_actions {
                    // Regret pruning: skip near-zero-probability actions after warmup
                    if strategy_buf[a] < 0.001 && iter > 1000 && iter % 1000 != 0 {
                        action_values_buf[a] = 0.0;
                        continue;
                    }
                    let av = cfr_traverse_flop(
                        &children[a],
                        traverser,
                        hand_idx,
                        flop_bucket,
                        turn_bucket,
                        river_bucket,
                        opp_reach,
                        oop_combos,
                        ip_combos,
                        oop_blockers,
                        ip_blockers,
                        flop_oop_buckets,
                        flop_ip_buckets,
                        turn_oop_buckets,
                        turn_ip_buckets,
                        river_oop_buckets,
                        river_ip_buckets,
                        oop_scores,
                        ip_scores,
                        valid_ip_for_oop,
                        valid_oop_for_ip,
                        flop_pot,
                        turn_template,
                        river_template,
                        flop_oop_cfr,
                        flop_ip_cfr,
                        turn_oop_cfr,
                        turn_ip_cfr,
                        river_oop_cfr,
                        river_ip_cfr,
                        strategy_buf,
                        action_values_buf,
                        iter,
                    );
                    action_values_buf[a] = av as f32;
                    node_value += strategy_buf[a] as f64 * av;
                }

                let reach_sum: f64 = opp_reach.iter().sum();
                let reach_prob = if reach_sum > 0.0 { 1.0f32 } else { 0.0f32 };

                let cfr_mut = match traverser {
                    Player::OOP => &mut *flop_oop_cfr,
                    Player::IP => &mut *flop_ip_cfr,
                };
                cfr_mut.update(
                    nid,
                    flop_bucket,
                    &action_values_buf[..num_actions],
                    node_value as f32,
                    reach_prob,
                );

                node_value
            } else {
                // Opponent node: weight by opponent's strategy per combo
                let num_opp = opp_reach.len();
                let opp_cfr = match traverser {
                    Player::OOP => &*flop_ip_cfr,
                    Player::IP => &*flop_oop_cfr,
                };
                let opp_buckets = match traverser {
                    Player::OOP => flop_ip_buckets,
                    Player::IP => flop_oop_buckets,
                };
                let opp_num_actions = opp_cfr.node_num_actions(nid) as usize;

                // Gather opponent strategies per combo (looked up by bucket)
                let mut opp_strats = vec![0.0f32; num_opp * opp_num_actions];
                for j in 0..num_opp {
                    if opp_reach[j] > 0.0 {
                        let bucket = opp_buckets[j] as usize;
                        opp_cfr.current_strategy(
                            nid,
                            bucket,
                            &mut opp_strats[j * opp_num_actions..(j + 1) * opp_num_actions],
                        );
                    }
                }

                let mut node_value = 0.0f64;
                for a in 0..num_actions {
                    let mut new_opp_reach = vec![0.0f64; num_opp];
                    for j in 0..num_opp {
                        if opp_reach[j] > 0.0 {
                            let sigma = opp_strats[j * opp_num_actions + a] as f64;
                            new_opp_reach[j] = opp_reach[j] * sigma;
                        }
                    }

                    node_value += cfr_traverse_flop(
                        &children[a],
                        traverser,
                        hand_idx,
                        flop_bucket,
                        turn_bucket,
                        river_bucket,
                        &new_opp_reach,
                        oop_combos,
                        ip_combos,
                        oop_blockers,
                        ip_blockers,
                        flop_oop_buckets,
                        flop_ip_buckets,
                        turn_oop_buckets,
                        turn_ip_buckets,
                        river_oop_buckets,
                        river_ip_buckets,
                        oop_scores,
                        ip_scores,
                        valid_ip_for_oop,
                        valid_oop_for_ip,
                        flop_pot,
                        turn_template,
                        river_template,
                        flop_oop_cfr,
                        flop_ip_cfr,
                        turn_oop_cfr,
                        turn_ip_cfr,
                        river_oop_cfr,
                        river_ip_cfr,
                        strategy_buf,
                        action_values_buf,
                        iter,
                    );
                }

                node_value
            }
        }
        TreeNode::Chance { .. } => {
            unreachable!("Flop tree should not contain chance nodes")
        }
    }
}

// ---------------------------------------------------------------------------
// MCCFR traversal: turn template
// ---------------------------------------------------------------------------

/// Traverse the turn template tree. All monetary values are scaled by `scale`.
/// At Showdown terminals, chain to the river template.
#[allow(clippy::too_many_arguments)]
fn cfr_traverse_turn_template(
    node: &TreeNode,
    traverser: Player,
    hand_idx: usize,
    turn_bucket: usize,
    river_bucket: usize,
    opp_reach: &[f64],
    oop_combos: &[Combo],
    ip_combos: &[Combo],
    turn_oop_buckets: &[u16],
    turn_ip_buckets: &[u16],
    river_oop_buckets: &[u16],
    river_ip_buckets: &[u16],
    oop_scores: &[u32],
    ip_scores: &[u32],
    valid_ip_for_oop: &[Vec<u16>],
    valid_oop_for_ip: &[Vec<u16>],
    scale: f64,
    river_template: &TreeNode,
    turn_oop_cfr: &mut FlatCfr,
    turn_ip_cfr: &mut FlatCfr,
    river_oop_cfr: &mut FlatCfr,
    river_ip_cfr: &mut FlatCfr,
    strategy_buf: &mut [f32],
    action_values_buf: &mut [f32],
    iter: usize,
) -> f64 {
    match node {
        TreeNode::Terminal {
            terminal_type,
            pot,
            invested,
            ..
        } => {
            let opp_reach_sum: f64 = opp_reach.iter().sum();
            if opp_reach_sum < 1e-10 {
                return 0.0;
            }

            match terminal_type {
                TerminalType::Fold { folder } => {
                    let my_invested = invested[traverser.index()] * scale;
                    if *folder == traverser {
                        -my_invested * opp_reach_sum
                    } else {
                        let pot_scaled = *pot * scale;
                        (pot_scaled - my_invested) * opp_reach_sum
                    }
                }
                TerminalType::Showdown => {
                    // Chain to river template
                    let river_scale = *pot * scale;
                    cfr_traverse_river_template(
                        river_template,
                        traverser,
                        hand_idx,
                        river_bucket,
                        opp_reach,
                        oop_combos,
                        ip_combos,
                        river_oop_buckets,
                        river_ip_buckets,
                        oop_scores,
                        ip_scores,
                        valid_ip_for_oop,
                        valid_oop_for_ip,
                        river_scale,
                        river_oop_cfr,
                        river_ip_cfr,
                        strategy_buf,
                        action_values_buf,
                        iter,
                    )
                }
            }
        }
        TreeNode::Action {
            node_id,
            player,
            children,
            actions,
            ..
        } => {
            let num_actions = actions.len();
            let nid = *node_id as usize;

            if *player == traverser {
                let cfr = match traverser {
                    Player::OOP => &*turn_oop_cfr,
                    Player::IP => &*turn_ip_cfr,
                };
                cfr.current_strategy(nid, turn_bucket, strategy_buf);

                let mut node_value = 0.0f64;
                for a in 0..num_actions {
                    // Regret pruning: skip near-zero-probability actions after warmup
                    if strategy_buf[a] < 0.001 && iter > 1000 && iter % 1000 != 0 {
                        action_values_buf[a] = 0.0;
                        continue;
                    }
                    let av = cfr_traverse_turn_template(
                        &children[a],
                        traverser,
                        hand_idx,
                        turn_bucket,
                        river_bucket,
                        opp_reach,
                        oop_combos,
                        ip_combos,
                        turn_oop_buckets,
                        turn_ip_buckets,
                        river_oop_buckets,
                        river_ip_buckets,
                        oop_scores,
                        ip_scores,
                        valid_ip_for_oop,
                        valid_oop_for_ip,
                        scale,
                        river_template,
                        turn_oop_cfr,
                        turn_ip_cfr,
                        river_oop_cfr,
                        river_ip_cfr,
                        strategy_buf,
                        action_values_buf,
                        iter,
                    );
                    action_values_buf[a] = av as f32;
                    node_value += strategy_buf[a] as f64 * av;
                }

                let reach_sum: f64 = opp_reach.iter().sum();
                let reach_prob = if reach_sum > 0.0 { 1.0f32 } else { 0.0f32 };

                let cfr_mut = match traverser {
                    Player::OOP => &mut *turn_oop_cfr,
                    Player::IP => &mut *turn_ip_cfr,
                };
                cfr_mut.update(
                    nid,
                    turn_bucket,
                    &action_values_buf[..num_actions],
                    node_value as f32,
                    reach_prob,
                );

                node_value
            } else {
                let num_opp = opp_reach.len();
                let opp_cfr = match traverser {
                    Player::OOP => &*turn_ip_cfr,
                    Player::IP => &*turn_oop_cfr,
                };
                let opp_buckets = match traverser {
                    Player::OOP => turn_ip_buckets,
                    Player::IP => turn_oop_buckets,
                };
                let opp_num_actions = opp_cfr.node_num_actions(nid) as usize;

                let mut opp_strats = vec![0.0f32; num_opp * opp_num_actions];
                for j in 0..num_opp {
                    if opp_reach[j] > 0.0 {
                        let bucket = opp_buckets[j] as usize;
                        opp_cfr.current_strategy(
                            nid,
                            bucket,
                            &mut opp_strats[j * opp_num_actions..(j + 1) * opp_num_actions],
                        );
                    }
                }

                let mut node_value = 0.0f64;
                for a in 0..num_actions {
                    let mut new_opp_reach = vec![0.0f64; num_opp];
                    for j in 0..num_opp {
                        if opp_reach[j] > 0.0 {
                            let sigma = opp_strats[j * opp_num_actions + a] as f64;
                            new_opp_reach[j] = opp_reach[j] * sigma;
                        }
                    }

                    node_value += cfr_traverse_turn_template(
                        &children[a],
                        traverser,
                        hand_idx,
                        turn_bucket,
                        river_bucket,
                        &new_opp_reach,
                        oop_combos,
                        ip_combos,
                        turn_oop_buckets,
                        turn_ip_buckets,
                        river_oop_buckets,
                        river_ip_buckets,
                        oop_scores,
                        ip_scores,
                        valid_ip_for_oop,
                        valid_oop_for_ip,
                        scale,
                        river_template,
                        turn_oop_cfr,
                        turn_ip_cfr,
                        river_oop_cfr,
                        river_ip_cfr,
                        strategy_buf,
                        action_values_buf,
                        iter,
                    );
                }

                node_value
            }
        }
        TreeNode::Chance { .. } => {
            unreachable!("Turn template should not contain chance nodes")
        }
    }
}

// ---------------------------------------------------------------------------
// MCCFR traversal: river template
// ---------------------------------------------------------------------------

/// Traverse the river template tree. At showdown, evaluate using precomputed scores.
/// All monetary values are scaled by `scale`.
#[allow(clippy::too_many_arguments)]
fn cfr_traverse_river_template(
    node: &TreeNode,
    traverser: Player,
    hand_idx: usize,
    river_bucket: usize,
    opp_reach: &[f64],
    oop_combos: &[Combo],
    ip_combos: &[Combo],
    river_oop_buckets: &[u16],
    river_ip_buckets: &[u16],
    oop_scores: &[u32],
    ip_scores: &[u32],
    valid_ip_for_oop: &[Vec<u16>],
    valid_oop_for_ip: &[Vec<u16>],
    scale: f64,
    river_oop_cfr: &mut FlatCfr,
    river_ip_cfr: &mut FlatCfr,
    strategy_buf: &mut [f32],
    action_values_buf: &mut [f32],
    iter: usize,
) -> f64 {
    match node {
        TreeNode::Terminal {
            terminal_type,
            pot,
            invested,
            ..
        } => {
            let opp_reach_sum: f64 = opp_reach.iter().sum();
            if opp_reach_sum < 1e-10 {
                return 0.0;
            }

            match terminal_type {
                TerminalType::Fold { folder } => {
                    let my_invested = invested[traverser.index()] * scale;
                    if *folder == traverser {
                        -my_invested * opp_reach_sum
                    } else {
                        let pot_scaled = *pot * scale;
                        (pot_scaled - my_invested) * opp_reach_sum
                    }
                }
                TerminalType::Showdown => {
                    // Actual showdown evaluation
                    let pot_scaled = *pot * scale;
                    let my_invested = invested[traverser.index()] * scale;
                    let win_payoff = pot_scaled - my_invested;
                    let lose_payoff = -my_invested;
                    let tie_payoff = pot_scaled / 2.0 - my_invested;
                    let mut value = 0.0;

                    match traverser {
                        Player::OOP => {
                            let my_score = oop_scores[hand_idx];
                            for &j in &valid_ip_for_oop[hand_idx] {
                                let j = j as usize;
                                if opp_reach[j] < 1e-10 {
                                    continue;
                                }
                                let opp_score = ip_scores[j];
                                let payoff = if my_score > opp_score {
                                    win_payoff
                                } else if my_score < opp_score {
                                    lose_payoff
                                } else {
                                    tie_payoff
                                };
                                value += opp_reach[j] * payoff;
                            }
                        }
                        Player::IP => {
                            let my_score = ip_scores[hand_idx];
                            for &i in &valid_oop_for_ip[hand_idx] {
                                let i = i as usize;
                                if opp_reach[i] < 1e-10 {
                                    continue;
                                }
                                let opp_score = oop_scores[i];
                                let payoff = if my_score > opp_score {
                                    win_payoff
                                } else if my_score < opp_score {
                                    lose_payoff
                                } else {
                                    tie_payoff
                                };
                                value += opp_reach[i] * payoff;
                            }
                        }
                    }

                    value
                }
            }
        }
        TreeNode::Action {
            node_id,
            player,
            children,
            actions,
            ..
        } => {
            let num_actions = actions.len();
            let nid = *node_id as usize;

            if *player == traverser {
                let cfr = match traverser {
                    Player::OOP => &*river_oop_cfr,
                    Player::IP => &*river_ip_cfr,
                };
                cfr.current_strategy(nid, river_bucket, strategy_buf);

                let mut node_value = 0.0f64;
                for a in 0..num_actions {
                    // Regret pruning: skip near-zero-probability actions after warmup
                    if strategy_buf[a] < 0.001 && iter > 1000 && iter % 1000 != 0 {
                        action_values_buf[a] = 0.0;
                        continue;
                    }
                    let av = cfr_traverse_river_template(
                        &children[a],
                        traverser,
                        hand_idx,
                        river_bucket,
                        opp_reach,
                        oop_combos,
                        ip_combos,
                        river_oop_buckets,
                        river_ip_buckets,
                        oop_scores,
                        ip_scores,
                        valid_ip_for_oop,
                        valid_oop_for_ip,
                        scale,
                        river_oop_cfr,
                        river_ip_cfr,
                        strategy_buf,
                        action_values_buf,
                        iter,
                    );
                    action_values_buf[a] = av as f32;
                    node_value += strategy_buf[a] as f64 * av;
                }

                let reach_sum: f64 = opp_reach.iter().sum();
                let reach_prob = if reach_sum > 0.0 { 1.0f32 } else { 0.0f32 };

                let cfr_mut = match traverser {
                    Player::OOP => &mut *river_oop_cfr,
                    Player::IP => &mut *river_ip_cfr,
                };
                cfr_mut.update(
                    nid,
                    river_bucket,
                    &action_values_buf[..num_actions],
                    node_value as f32,
                    reach_prob,
                );

                node_value
            } else {
                let num_opp = opp_reach.len();
                let opp_cfr = match traverser {
                    Player::OOP => &*river_ip_cfr,
                    Player::IP => &*river_oop_cfr,
                };
                let opp_buckets = match traverser {
                    Player::OOP => river_ip_buckets,
                    Player::IP => river_oop_buckets,
                };
                let opp_num_actions = opp_cfr.node_num_actions(nid) as usize;

                let mut opp_strats = vec![0.0f32; num_opp * opp_num_actions];
                for j in 0..num_opp {
                    if opp_reach[j] > 0.0 {
                        let bucket = opp_buckets[j] as usize;
                        opp_cfr.current_strategy(
                            nid,
                            bucket,
                            &mut opp_strats[j * opp_num_actions..(j + 1) * opp_num_actions],
                        );
                    }
                }

                let mut node_value = 0.0f64;
                for a in 0..num_actions {
                    let mut new_opp_reach = vec![0.0f64; num_opp];
                    for j in 0..num_opp {
                        if opp_reach[j] > 0.0 {
                            let sigma = opp_strats[j * opp_num_actions + a] as f64;
                            new_opp_reach[j] = opp_reach[j] * sigma;
                        }
                    }

                    node_value += cfr_traverse_river_template(
                        &children[a],
                        traverser,
                        hand_idx,
                        river_bucket,
                        &new_opp_reach,
                        oop_combos,
                        ip_combos,
                        river_oop_buckets,
                        river_ip_buckets,
                        oop_scores,
                        ip_scores,
                        valid_ip_for_oop,
                        valid_oop_for_ip,
                        scale,
                        river_oop_cfr,
                        river_ip_cfr,
                        strategy_buf,
                        action_values_buf,
                        iter,
                    );
                }

                node_value
            }
        }
        TreeNode::Chance { .. } => {
            unreachable!("River template should not contain chance nodes")
        }
    }
}

// ---------------------------------------------------------------------------
// Parallel traversal: readonly + collected updates
// ---------------------------------------------------------------------------

/// A collected regret update for deferred application after parallel traversal.
struct RegretUpdate {
    /// 0 = flop, 1 = turn, 2 = river
    street: u8,
    node_id: usize,
    bucket: usize,
    action_values: Vec<f32>,
    node_value: f32,
    reach_prob: f32,
}

/// Readonly flop traversal that collects RegretUpdates instead of mutating CFR.
#[allow(clippy::too_many_arguments)]
fn cfr_traverse_flop_ro(
    node: &TreeNode, traverser: Player, hand_idx: usize,
    flop_bucket: usize, turn_bucket: usize, river_bucket: usize,
    opp_reach: &[f64], oop_combos: &[Combo], ip_combos: &[Combo],
    oop_blockers: &[[bool; 52]], ip_blockers: &[[bool; 52]],
    flop_oop_buckets: &[u16], flop_ip_buckets: &[u16],
    turn_oop_buckets: &[u16], turn_ip_buckets: &[u16],
    river_oop_buckets: &[u16], river_ip_buckets: &[u16],
    oop_scores: &[u32], ip_scores: &[u32],
    valid_ip_for_oop: &[Vec<u16>], valid_oop_for_ip: &[Vec<u16>],
    flop_pot: f64, turn_template: &TreeNode, river_template: &TreeNode,
    flop_oop_cfr: &FlatCfr, flop_ip_cfr: &FlatCfr,
    turn_oop_cfr: &FlatCfr, turn_ip_cfr: &FlatCfr,
    river_oop_cfr: &FlatCfr, river_ip_cfr: &FlatCfr,
    updates: &mut Vec<RegretUpdate>, iter: usize,
) -> f64 {
    match node {
        TreeNode::Terminal { terminal_type, pot, invested, .. } => {
            let opp_reach_sum: f64 = opp_reach.iter().sum();
            if opp_reach_sum < 1e-10 { return 0.0; }
            let my_invested = invested[traverser.index()];
            match terminal_type {
                TerminalType::Fold { folder } => {
                    if *folder == traverser { -my_invested * opp_reach_sum }
                    else { (*pot - my_invested) * opp_reach_sum }
                }
                TerminalType::Showdown => {
                    let turn_scale = *pot;
                    let turn_value = cfr_traverse_turn_template_ro(
                        turn_template, traverser, hand_idx, turn_bucket, river_bucket,
                        opp_reach, oop_combos, ip_combos,
                        turn_oop_buckets, turn_ip_buckets,
                        river_oop_buckets, river_ip_buckets,
                        oop_scores, ip_scores, valid_ip_for_oop, valid_oop_for_ip,
                        turn_scale, river_template,
                        turn_oop_cfr, turn_ip_cfr, river_oop_cfr, river_ip_cfr,
                        updates, iter,
                    );
                    turn_value - my_invested * opp_reach_sum
                }
            }
        }
        TreeNode::Action { node_id, player, children, actions, .. } => {
            let num_actions = actions.len();
            let nid = *node_id as usize;
            if *player == traverser {
                let cfr = match traverser {
                    Player::OOP => flop_oop_cfr,
                    Player::IP => flop_ip_cfr,
                };
                let mut strategy = vec![0.0f32; num_actions];
                cfr.current_strategy(nid, flop_bucket, &mut strategy);
                let mut node_value = 0.0f64;
                let mut action_vals = vec![0.0f32; num_actions];
                for a in 0..num_actions {
                    if strategy[a] < 0.001 && iter > 1000 && iter % 1000 != 0 { continue; }
                    let av = cfr_traverse_flop_ro(
                        &children[a], traverser, hand_idx,
                        flop_bucket, turn_bucket, river_bucket,
                        opp_reach, oop_combos, ip_combos,
                        oop_blockers, ip_blockers,
                        flop_oop_buckets, flop_ip_buckets,
                        turn_oop_buckets, turn_ip_buckets,
                        river_oop_buckets, river_ip_buckets,
                        oop_scores, ip_scores,
                        valid_ip_for_oop, valid_oop_for_ip,
                        flop_pot, turn_template, river_template,
                        flop_oop_cfr, flop_ip_cfr,
                        turn_oop_cfr, turn_ip_cfr,
                        river_oop_cfr, river_ip_cfr,
                        updates, iter,
                    );
                    action_vals[a] = av as f32;
                    node_value += strategy[a] as f64 * av;
                }
                let reach_sum: f64 = opp_reach.iter().sum();
                let reach_prob = if reach_sum > 0.0 { 1.0f32 } else { 0.0f32 };
                updates.push(RegretUpdate {
                    street: 0, node_id: nid, bucket: flop_bucket,
                    action_values: action_vals, node_value: node_value as f32, reach_prob,
                });
                node_value
            } else {
                let num_opp = opp_reach.len();
                let opp_cfr = match traverser {
                    Player::OOP => flop_ip_cfr,
                    Player::IP => flop_oop_cfr,
                };
                let opp_buckets = match traverser {
                    Player::OOP => flop_ip_buckets,
                    Player::IP => flop_oop_buckets,
                };
                let opp_num_actions = opp_cfr.node_num_actions(nid) as usize;
                let mut opp_strats = vec![0.0f32; num_opp * opp_num_actions];
                for j in 0..num_opp {
                    if opp_reach[j] > 0.0 {
                        let bucket = opp_buckets[j] as usize;
                        opp_cfr.current_strategy(nid, bucket, &mut opp_strats[j * opp_num_actions..(j + 1) * opp_num_actions]);
                    }
                }
                let mut node_value = 0.0f64;
                for a in 0..num_actions {
                    let mut new_opp_reach = vec![0.0f64; num_opp];
                    for j in 0..num_opp {
                        if opp_reach[j] > 0.0 {
                            new_opp_reach[j] = opp_reach[j] * opp_strats[j * opp_num_actions + a] as f64;
                        }
                    }
                    node_value += cfr_traverse_flop_ro(
                        &children[a], traverser, hand_idx,
                        flop_bucket, turn_bucket, river_bucket,
                        &new_opp_reach, oop_combos, ip_combos,
                        oop_blockers, ip_blockers,
                        flop_oop_buckets, flop_ip_buckets,
                        turn_oop_buckets, turn_ip_buckets,
                        river_oop_buckets, river_ip_buckets,
                        oop_scores, ip_scores,
                        valid_ip_for_oop, valid_oop_for_ip,
                        flop_pot, turn_template, river_template,
                        flop_oop_cfr, flop_ip_cfr,
                        turn_oop_cfr, turn_ip_cfr,
                        river_oop_cfr, river_ip_cfr,
                        updates, iter,
                    );
                }
                node_value
            }
        }
        TreeNode::Chance { .. } => unreachable!("Flop tree should not contain chance nodes"),
    }
}

/// Readonly turn template traversal.
#[allow(clippy::too_many_arguments)]
fn cfr_traverse_turn_template_ro(
    node: &TreeNode, traverser: Player, hand_idx: usize,
    turn_bucket: usize, river_bucket: usize,
    opp_reach: &[f64], oop_combos: &[Combo], ip_combos: &[Combo],
    turn_oop_buckets: &[u16], turn_ip_buckets: &[u16],
    river_oop_buckets: &[u16], river_ip_buckets: &[u16],
    oop_scores: &[u32], ip_scores: &[u32],
    valid_ip_for_oop: &[Vec<u16>], valid_oop_for_ip: &[Vec<u16>],
    scale: f64, river_template: &TreeNode,
    turn_oop_cfr: &FlatCfr, turn_ip_cfr: &FlatCfr,
    river_oop_cfr: &FlatCfr, river_ip_cfr: &FlatCfr,
    updates: &mut Vec<RegretUpdate>, iter: usize,
) -> f64 {
    match node {
        TreeNode::Terminal { terminal_type, pot, invested, .. } => {
            let opp_reach_sum: f64 = opp_reach.iter().sum();
            if opp_reach_sum < 1e-10 { return 0.0; }
            match terminal_type {
                TerminalType::Fold { folder } => {
                    let my_invested = invested[traverser.index()] * scale;
                    if *folder == traverser { -my_invested * opp_reach_sum }
                    else { (*pot * scale - my_invested) * opp_reach_sum }
                }
                TerminalType::Showdown => {
                    let river_scale = *pot * scale;
                    cfr_traverse_river_template_ro(
                        river_template, traverser, hand_idx, river_bucket,
                        opp_reach, oop_combos, ip_combos,
                        river_oop_buckets, river_ip_buckets,
                        oop_scores, ip_scores, valid_ip_for_oop, valid_oop_for_ip,
                        river_scale, river_oop_cfr, river_ip_cfr, updates, iter,
                    )
                }
            }
        }
        TreeNode::Action { node_id, player, children, actions, .. } => {
            let num_actions = actions.len();
            let nid = *node_id as usize;
            if *player == traverser {
                let cfr = match traverser {
                    Player::OOP => turn_oop_cfr,
                    Player::IP => turn_ip_cfr,
                };
                let mut strategy = vec![0.0f32; num_actions];
                cfr.current_strategy(nid, turn_bucket, &mut strategy);
                let mut node_value = 0.0f64;
                let mut action_vals = vec![0.0f32; num_actions];
                for a in 0..num_actions {
                    if strategy[a] < 0.001 && iter > 1000 && iter % 1000 != 0 { continue; }
                    let av = cfr_traverse_turn_template_ro(
                        &children[a], traverser, hand_idx, turn_bucket, river_bucket,
                        opp_reach, oop_combos, ip_combos,
                        turn_oop_buckets, turn_ip_buckets,
                        river_oop_buckets, river_ip_buckets,
                        oop_scores, ip_scores, valid_ip_for_oop, valid_oop_for_ip,
                        scale, river_template,
                        turn_oop_cfr, turn_ip_cfr, river_oop_cfr, river_ip_cfr,
                        updates, iter,
                    );
                    action_vals[a] = av as f32;
                    node_value += strategy[a] as f64 * av;
                }
                let reach_sum: f64 = opp_reach.iter().sum();
                let reach_prob = if reach_sum > 0.0 { 1.0f32 } else { 0.0f32 };
                updates.push(RegretUpdate {
                    street: 1, node_id: nid, bucket: turn_bucket,
                    action_values: action_vals, node_value: node_value as f32, reach_prob,
                });
                node_value
            } else {
                let num_opp = opp_reach.len();
                let opp_cfr = match traverser {
                    Player::OOP => turn_ip_cfr,
                    Player::IP => turn_oop_cfr,
                };
                let opp_buckets = match traverser {
                    Player::OOP => turn_ip_buckets,
                    Player::IP => turn_oop_buckets,
                };
                let opp_num_actions = opp_cfr.node_num_actions(nid) as usize;
                let mut opp_strats = vec![0.0f32; num_opp * opp_num_actions];
                for j in 0..num_opp {
                    if opp_reach[j] > 0.0 {
                        let bucket = opp_buckets[j] as usize;
                        opp_cfr.current_strategy(nid, bucket, &mut opp_strats[j * opp_num_actions..(j + 1) * opp_num_actions]);
                    }
                }
                let mut node_value = 0.0f64;
                for a in 0..num_actions {
                    let mut new_opp_reach = vec![0.0f64; num_opp];
                    for j in 0..num_opp {
                        if opp_reach[j] > 0.0 {
                            new_opp_reach[j] = opp_reach[j] * opp_strats[j * opp_num_actions + a] as f64;
                        }
                    }
                    node_value += cfr_traverse_turn_template_ro(
                        &children[a], traverser, hand_idx, turn_bucket, river_bucket,
                        &new_opp_reach, oop_combos, ip_combos,
                        turn_oop_buckets, turn_ip_buckets,
                        river_oop_buckets, river_ip_buckets,
                        oop_scores, ip_scores, valid_ip_for_oop, valid_oop_for_ip,
                        scale, river_template,
                        turn_oop_cfr, turn_ip_cfr, river_oop_cfr, river_ip_cfr,
                        updates, iter,
                    );
                }
                node_value
            }
        }
        TreeNode::Chance { .. } => unreachable!("Turn template should not contain chance nodes"),
    }
}

/// Readonly river template traversal.
#[allow(clippy::too_many_arguments)]
fn cfr_traverse_river_template_ro(
    node: &TreeNode, traverser: Player, hand_idx: usize,
    river_bucket: usize, opp_reach: &[f64],
    oop_combos: &[Combo], ip_combos: &[Combo],
    river_oop_buckets: &[u16], river_ip_buckets: &[u16],
    oop_scores: &[u32], ip_scores: &[u32],
    valid_ip_for_oop: &[Vec<u16>], valid_oop_for_ip: &[Vec<u16>],
    scale: f64, river_oop_cfr: &FlatCfr, river_ip_cfr: &FlatCfr,
    updates: &mut Vec<RegretUpdate>, iter: usize,
) -> f64 {
    match node {
        TreeNode::Terminal { terminal_type, pot, invested, .. } => {
            let opp_reach_sum: f64 = opp_reach.iter().sum();
            if opp_reach_sum < 1e-10 { return 0.0; }
            match terminal_type {
                TerminalType::Fold { folder } => {
                    let my_invested = invested[traverser.index()] * scale;
                    if *folder == traverser { -my_invested * opp_reach_sum }
                    else { (*pot * scale - my_invested) * opp_reach_sum }
                }
                TerminalType::Showdown => {
                    let pot_scaled = *pot * scale;
                    let my_invested = invested[traverser.index()] * scale;
                    let win_payoff = pot_scaled - my_invested;
                    let lose_payoff = -my_invested;
                    let tie_payoff = pot_scaled / 2.0 - my_invested;
                    let mut value = 0.0;
                    match traverser {
                        Player::OOP => {
                            let my_score = oop_scores[hand_idx];
                            for &j in &valid_ip_for_oop[hand_idx] {
                                let j = j as usize;
                                if opp_reach[j] < 1e-10 { continue; }
                                let opp_score = ip_scores[j];
                                value += opp_reach[j] * if my_score > opp_score { win_payoff } else if my_score < opp_score { lose_payoff } else { tie_payoff };
                            }
                        }
                        Player::IP => {
                            let my_score = ip_scores[hand_idx];
                            for &i in &valid_oop_for_ip[hand_idx] {
                                let i = i as usize;
                                if opp_reach[i] < 1e-10 { continue; }
                                let opp_score = oop_scores[i];
                                value += opp_reach[i] * if my_score > opp_score { win_payoff } else if my_score < opp_score { lose_payoff } else { tie_payoff };
                            }
                        }
                    }
                    value
                }
            }
        }
        TreeNode::Action { node_id, player, children, actions, .. } => {
            let num_actions = actions.len();
            let nid = *node_id as usize;
            if *player == traverser {
                let cfr = match traverser {
                    Player::OOP => river_oop_cfr,
                    Player::IP => river_ip_cfr,
                };
                let mut strategy = vec![0.0f32; num_actions];
                cfr.current_strategy(nid, river_bucket, &mut strategy);
                let mut node_value = 0.0f64;
                let mut action_vals = vec![0.0f32; num_actions];
                for a in 0..num_actions {
                    if strategy[a] < 0.001 && iter > 1000 && iter % 1000 != 0 { continue; }
                    let av = cfr_traverse_river_template_ro(
                        &children[a], traverser, hand_idx, river_bucket,
                        opp_reach, oop_combos, ip_combos,
                        river_oop_buckets, river_ip_buckets,
                        oop_scores, ip_scores, valid_ip_for_oop, valid_oop_for_ip,
                        scale, river_oop_cfr, river_ip_cfr, updates, iter,
                    );
                    action_vals[a] = av as f32;
                    node_value += strategy[a] as f64 * av;
                }
                let reach_sum: f64 = opp_reach.iter().sum();
                let reach_prob = if reach_sum > 0.0 { 1.0f32 } else { 0.0f32 };
                updates.push(RegretUpdate {
                    street: 2, node_id: nid, bucket: river_bucket,
                    action_values: action_vals, node_value: node_value as f32, reach_prob,
                });
                node_value
            } else {
                let num_opp = opp_reach.len();
                let opp_cfr = match traverser {
                    Player::OOP => river_ip_cfr,
                    Player::IP => river_oop_cfr,
                };
                let opp_buckets = match traverser {
                    Player::OOP => river_ip_buckets,
                    Player::IP => river_oop_buckets,
                };
                let opp_num_actions = opp_cfr.node_num_actions(nid) as usize;
                let mut opp_strats = vec![0.0f32; num_opp * opp_num_actions];
                for j in 0..num_opp {
                    if opp_reach[j] > 0.0 {
                        let bucket = opp_buckets[j] as usize;
                        opp_cfr.current_strategy(nid, bucket, &mut opp_strats[j * opp_num_actions..(j + 1) * opp_num_actions]);
                    }
                }
                let mut node_value = 0.0f64;
                for a in 0..num_actions {
                    let mut new_opp_reach = vec![0.0f64; num_opp];
                    for j in 0..num_opp {
                        if opp_reach[j] > 0.0 {
                            new_opp_reach[j] = opp_reach[j] * opp_strats[j * opp_num_actions + a] as f64;
                        }
                    }
                    node_value += cfr_traverse_river_template_ro(
                        &children[a], traverser, hand_idx, river_bucket,
                        &new_opp_reach, oop_combos, ip_combos,
                        river_oop_buckets, river_ip_buckets,
                        oop_scores, ip_scores, valid_ip_for_oop, valid_oop_for_ip,
                        scale, river_oop_cfr, river_ip_cfr, updates, iter,
                    );
                }
                node_value
            }
        }
        TreeNode::Chance { .. } => unreachable!("River template should not contain chance nodes"),
    }
}

// ---------------------------------------------------------------------------
// Exploitability (Monte Carlo estimate)
// ---------------------------------------------------------------------------

/// Estimate exploitability via Monte Carlo best-response sampling.
#[allow(clippy::too_many_arguments)]
fn estimate_exploitability(
    flop_tree: &TreeNode,
    turn_template: &TreeNode,
    river_template: &TreeNode,
    flop_oop_cfr: &FlatCfr,
    flop_ip_cfr: &FlatCfr,
    turn_oop_cfr: &FlatCfr,
    turn_ip_cfr: &FlatCfr,
    river_oop_cfr: &FlatCfr,
    river_ip_cfr: &FlatCfr,
    oop_combos: &[Combo],
    ip_combos: &[Combo],
    oop_blockers: &[[bool; 52]],
    ip_blockers: &[[bool; 52]],
    flop_oop_buckets: &[u16],
    flop_ip_buckets: &[u16],
    valid_ip_for_oop: &[Vec<u16>],
    valid_oop_for_ip: &[Vec<u16>],
    board: &[u8],
    starting_pot: f64,
    num_buckets: usize,
) -> f64 {
    let remaining = remaining_deck(board);
    let num_remaining = remaining.len();
    let num_samples = 100;
    let mut rng = rand::thread_rng();

    let oop_pairs: Vec<(u8, u8)> = oop_combos.iter().map(|c| (c.0, c.1)).collect();
    let ip_pairs: Vec<(u8, u8)> = ip_combos.iter().map(|c| (c.0, c.1)).collect();

    // Precompute turn buckets for exploitability estimation
    let turn_bucket_table: Vec<(Vec<u16>, Vec<u16>)> = remaining
        .par_iter()
        .map(|&turn_card| {
            let turn_board = [board[0], board[1], board[2], turn_card];
            let t_oop = assign_buckets(&oop_pairs, &turn_board, num_buckets, 200);
            let t_ip = assign_buckets(&ip_pairs, &turn_board, num_buckets, 200);
            (t_oop, t_ip)
        })
        .collect();

    // Precompute river data for exploitability estimation
    let num_runouts = num_remaining * (num_remaining - 1);
    let river_data: Vec<((Vec<u16>, Vec<u16>), (Vec<u32>, Vec<u32>))> = (0..num_runouts)
        .into_par_iter()
        .map(|runout_idx| {
            let turn_idx = runout_idx / (num_remaining - 1);
            let river_adj = runout_idx % (num_remaining - 1);
            let river_idx = if river_adj >= turn_idx {
                river_adj + 1
            } else {
                river_adj
            };
            let turn_card = remaining[turn_idx];
            let river_card = remaining[river_idx];
            let river_board = [board[0], board[1], board[2], turn_card, river_card];
            let r_oop = assign_buckets(&oop_pairs, &river_board, num_buckets, 0);
            let r_ip = assign_buckets(&ip_pairs, &river_board, num_buckets, 0);
            let s_oop: Vec<u32> = oop_pairs
                .iter()
                .map(|&(c0, c1)| {
                    evaluate_fast(&[
                        c0, c1, river_board[0], river_board[1], river_board[2],
                        river_board[3], river_board[4],
                    ])
                })
                .collect();
            let s_ip: Vec<u32> = ip_pairs
                .iter()
                .map(|&(c0, c1)| {
                    evaluate_fast(&[
                        c0, c1, river_board[0], river_board[1], river_board[2],
                        river_board[3], river_board[4],
                    ])
                })
                .collect();
            ((r_oop, r_ip), (s_oop, s_ip))
        })
        .collect();

    let mut oop_total_gain = 0.0;
    let mut ip_total_gain = 0.0;
    let mut sample_count = 0;

    for _ in 0..num_samples {
        let turn_raw_idx = rng.gen_range(0..num_remaining);
        let turn_card = remaining[turn_raw_idx];
        let river_raw_idx = loop {
            let ri = rng.gen_range(0..num_remaining);
            if ri != turn_raw_idx {
                break ri;
            }
        };
        let river_card = remaining[river_raw_idx];

        let (turn_oop_buckets, turn_ip_buckets) = &turn_bucket_table[turn_raw_idx];
        let runout_idx = turn_raw_idx * (num_remaining - 1)
            + if river_raw_idx > turn_raw_idx {
                river_raw_idx - 1
            } else {
                river_raw_idx
            };
        let ((river_oop_buckets, river_ip_buckets), (oop_scores, ip_scores)) =
            &river_data[runout_idx];

        let mut strat_buf = vec![0.0f32; 16];

        // Compute BR and avg value for OOP
        for h in 0..oop_combos.len() {
            if oop_blockers[h][turn_card as usize] || oop_blockers[h][river_card as usize] {
                continue;
            }
            let mut opp_reach = vec![0.0f64; ip_combos.len()];
            for &j in &valid_ip_for_oop[h] {
                let j = j as usize;
                if !ip_blockers[j][turn_card as usize] && !ip_blockers[j][river_card as usize] {
                    opp_reach[j] = 1.0;
                }
            }

            let flop_bucket = flop_oop_buckets[h] as usize;
            let turn_bucket = turn_oop_buckets[h] as usize;
            let river_bucket = river_oop_buckets[h] as usize;

            let br_val = br_traverse_flop(
                flop_tree, Player::OOP, h, flop_bucket, turn_bucket, river_bucket,
                &opp_reach, oop_combos, ip_combos,
                flop_oop_buckets, flop_ip_buckets,
                turn_oop_buckets, turn_ip_buckets,
                river_oop_buckets, river_ip_buckets,
                oop_scores, ip_scores, valid_ip_for_oop, valid_oop_for_ip,
                starting_pot, turn_template, river_template,
                flop_oop_cfr, flop_ip_cfr, turn_oop_cfr, turn_ip_cfr,
                river_oop_cfr, river_ip_cfr, &mut strat_buf, true,
            );
            let avg_val = br_traverse_flop(
                flop_tree, Player::OOP, h, flop_bucket, turn_bucket, river_bucket,
                &opp_reach, oop_combos, ip_combos,
                flop_oop_buckets, flop_ip_buckets,
                turn_oop_buckets, turn_ip_buckets,
                river_oop_buckets, river_ip_buckets,
                oop_scores, ip_scores, valid_ip_for_oop, valid_oop_for_ip,
                starting_pot, turn_template, river_template,
                flop_oop_cfr, flop_ip_cfr, turn_oop_cfr, turn_ip_cfr,
                river_oop_cfr, river_ip_cfr, &mut strat_buf, false,
            );
            oop_total_gain += br_val - avg_val;
            sample_count += 1;
        }

        // Compute BR and avg value for IP
        for h in 0..ip_combos.len() {
            if ip_blockers[h][turn_card as usize] || ip_blockers[h][river_card as usize] {
                continue;
            }
            let mut opp_reach = vec![0.0f64; oop_combos.len()];
            for &i in &valid_oop_for_ip[h] {
                let i = i as usize;
                if !oop_blockers[i][turn_card as usize] && !oop_blockers[i][river_card as usize] {
                    opp_reach[i] = 1.0;
                }
            }

            let flop_bucket = flop_ip_buckets[h] as usize;
            let turn_bucket = turn_ip_buckets[h] as usize;
            let river_bucket = river_ip_buckets[h] as usize;

            let br_val = br_traverse_flop(
                flop_tree, Player::IP, h, flop_bucket, turn_bucket, river_bucket,
                &opp_reach, oop_combos, ip_combos,
                flop_oop_buckets, flop_ip_buckets,
                turn_oop_buckets, turn_ip_buckets,
                river_oop_buckets, river_ip_buckets,
                oop_scores, ip_scores, valid_ip_for_oop, valid_oop_for_ip,
                starting_pot, turn_template, river_template,
                flop_oop_cfr, flop_ip_cfr, turn_oop_cfr, turn_ip_cfr,
                river_oop_cfr, river_ip_cfr, &mut strat_buf, true,
            );
            let avg_val = br_traverse_flop(
                flop_tree, Player::IP, h, flop_bucket, turn_bucket, river_bucket,
                &opp_reach, oop_combos, ip_combos,
                flop_oop_buckets, flop_ip_buckets,
                turn_oop_buckets, turn_ip_buckets,
                river_oop_buckets, river_ip_buckets,
                oop_scores, ip_scores, valid_ip_for_oop, valid_oop_for_ip,
                starting_pot, turn_template, river_template,
                flop_oop_cfr, flop_ip_cfr, turn_oop_cfr, turn_ip_cfr,
                river_oop_cfr, river_ip_cfr, &mut strat_buf, false,
            );
            ip_total_gain += br_val - avg_val;
        }
    }

    if sample_count > 0 {
        (oop_total_gain + ip_total_gain) / (2.0 * sample_count as f64)
    } else {
        0.0
    }
}

// ---------------------------------------------------------------------------
// Best-response traversal for exploitability
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
fn br_traverse_flop(
    node: &TreeNode,
    br_player: Player,
    hand_idx: usize,
    flop_bucket: usize,
    turn_bucket: usize,
    river_bucket: usize,
    opp_reach: &[f64],
    oop_combos: &[Combo],
    ip_combos: &[Combo],
    flop_oop_buckets: &[u16],
    flop_ip_buckets: &[u16],
    turn_oop_buckets: &[u16],
    turn_ip_buckets: &[u16],
    river_oop_buckets: &[u16],
    river_ip_buckets: &[u16],
    oop_scores: &[u32],
    ip_scores: &[u32],
    valid_ip_for_oop: &[Vec<u16>],
    valid_oop_for_ip: &[Vec<u16>],
    flop_pot: f64,
    turn_template: &TreeNode,
    river_template: &TreeNode,
    flop_oop_cfr: &FlatCfr,
    flop_ip_cfr: &FlatCfr,
    turn_oop_cfr: &FlatCfr,
    turn_ip_cfr: &FlatCfr,
    river_oop_cfr: &FlatCfr,
    river_ip_cfr: &FlatCfr,
    strat_buf: &mut [f32],
    is_br: bool,
) -> f64 {
    match node {
        TreeNode::Terminal {
            terminal_type,
            pot,
            invested,
            ..
        } => {
            let opp_reach_sum: f64 = opp_reach.iter().sum();
            if opp_reach_sum < 1e-10 {
                return 0.0;
            }
            let my_invested = invested[br_player.index()];
            match terminal_type {
                TerminalType::Fold { folder } => {
                    if *folder == br_player {
                        -my_invested * opp_reach_sum
                    } else {
                        (*pot - my_invested) * opp_reach_sum
                    }
                }
                TerminalType::Showdown => {
                    let turn_scale = *pot;
                    let turn_val = br_traverse_turn_template(
                        turn_template,
                        br_player,
                        hand_idx,
                        turn_bucket,
                        river_bucket,
                        opp_reach,
                        oop_combos,
                        ip_combos,
                        turn_oop_buckets,
                        turn_ip_buckets,
                        river_oop_buckets,
                        river_ip_buckets,
                        oop_scores,
                        ip_scores,
                        valid_ip_for_oop,
                        valid_oop_for_ip,
                        turn_scale,
                        river_template,
                        turn_oop_cfr,
                        turn_ip_cfr,
                        river_oop_cfr,
                        river_ip_cfr,
                        strat_buf,
                        is_br,
                    );
                    turn_val - my_invested * opp_reach_sum
                }
            }
        }
        TreeNode::Action {
            node_id,
            player,
            children,
            actions,
            ..
        } => {
            let num_actions = actions.len();
            let nid = *node_id as usize;

            if *player == br_player {
                if is_br {
                    let mut best = f64::NEG_INFINITY;
                    for a in 0..num_actions {
                        let v = br_traverse_flop(
                            &children[a], br_player, hand_idx, flop_bucket, turn_bucket, river_bucket,
                            opp_reach, oop_combos, ip_combos,
                            flop_oop_buckets, flop_ip_buckets,
                            turn_oop_buckets, turn_ip_buckets,
                            river_oop_buckets, river_ip_buckets,
                            oop_scores, ip_scores, valid_ip_for_oop, valid_oop_for_ip,
                            flop_pot, turn_template, river_template,
                            flop_oop_cfr, flop_ip_cfr, turn_oop_cfr, turn_ip_cfr,
                            river_oop_cfr, river_ip_cfr, strat_buf, is_br,
                        );
                        if v > best { best = v; }
                    }
                    best
                } else {
                    let cfr = match br_player {
                        Player::OOP => flop_oop_cfr,
                        Player::IP => flop_ip_cfr,
                    };
                    cfr.average_strategy(nid, flop_bucket, strat_buf);
                    let mut node_value = 0.0;
                    for a in 0..num_actions {
                        let v = br_traverse_flop(
                            &children[a], br_player, hand_idx, flop_bucket, turn_bucket, river_bucket,
                            opp_reach, oop_combos, ip_combos,
                            flop_oop_buckets, flop_ip_buckets,
                            turn_oop_buckets, turn_ip_buckets,
                            river_oop_buckets, river_ip_buckets,
                            oop_scores, ip_scores, valid_ip_for_oop, valid_oop_for_ip,
                            flop_pot, turn_template, river_template,
                            flop_oop_cfr, flop_ip_cfr, turn_oop_cfr, turn_ip_cfr,
                            river_oop_cfr, river_ip_cfr, strat_buf, is_br,
                        );
                        node_value += strat_buf[a] as f64 * v;
                    }
                    node_value
                }
            } else {
                let opp_cfr = match br_player {
                    Player::OOP => flop_ip_cfr,
                    Player::IP => flop_oop_cfr,
                };
                let opp_buckets = match br_player {
                    Player::OOP => flop_ip_buckets,
                    Player::IP => flop_oop_buckets,
                };
                let num_opp = opp_reach.len();
                let mut node_value = 0.0;

                for a in 0..num_actions {
                    let mut new_opp_reach = vec![0.0f64; num_opp];
                    for j in 0..num_opp {
                        if opp_reach[j] > 0.0 {
                            let bucket = opp_buckets[j] as usize;
                            opp_cfr.average_strategy(nid, bucket, strat_buf);
                            new_opp_reach[j] = opp_reach[j] * strat_buf[a] as f64;
                        }
                    }
                    node_value += br_traverse_flop(
                        &children[a], br_player, hand_idx, flop_bucket, turn_bucket, river_bucket,
                        &new_opp_reach, oop_combos, ip_combos,
                        flop_oop_buckets, flop_ip_buckets,
                        turn_oop_buckets, turn_ip_buckets,
                        river_oop_buckets, river_ip_buckets,
                        oop_scores, ip_scores, valid_ip_for_oop, valid_oop_for_ip,
                        flop_pot, turn_template, river_template,
                        flop_oop_cfr, flop_ip_cfr, turn_oop_cfr, turn_ip_cfr,
                        river_oop_cfr, river_ip_cfr, strat_buf, is_br,
                    );
                }
                node_value
            }
        }
        TreeNode::Chance { .. } => unreachable!("Flop tree should not contain chance nodes"),
    }
}

#[allow(clippy::too_many_arguments)]
fn br_traverse_turn_template(
    node: &TreeNode,
    br_player: Player,
    hand_idx: usize,
    turn_bucket: usize,
    river_bucket: usize,
    opp_reach: &[f64],
    oop_combos: &[Combo],
    ip_combos: &[Combo],
    turn_oop_buckets: &[u16],
    turn_ip_buckets: &[u16],
    river_oop_buckets: &[u16],
    river_ip_buckets: &[u16],
    oop_scores: &[u32],
    ip_scores: &[u32],
    valid_ip_for_oop: &[Vec<u16>],
    valid_oop_for_ip: &[Vec<u16>],
    scale: f64,
    river_template: &TreeNode,
    turn_oop_cfr: &FlatCfr,
    turn_ip_cfr: &FlatCfr,
    river_oop_cfr: &FlatCfr,
    river_ip_cfr: &FlatCfr,
    strat_buf: &mut [f32],
    is_br: bool,
) -> f64 {
    match node {
        TreeNode::Terminal {
            terminal_type,
            pot,
            invested,
            ..
        } => {
            let opp_reach_sum: f64 = opp_reach.iter().sum();
            if opp_reach_sum < 1e-10 {
                return 0.0;
            }
            match terminal_type {
                TerminalType::Fold { folder } => {
                    let my_invested = invested[br_player.index()] * scale;
                    if *folder == br_player {
                        -my_invested * opp_reach_sum
                    } else {
                        (*pot * scale - my_invested) * opp_reach_sum
                    }
                }
                TerminalType::Showdown => {
                    let river_scale = *pot * scale;
                    br_traverse_river_template(
                        river_template, br_player, hand_idx, river_bucket,
                        opp_reach, oop_combos, ip_combos,
                        river_oop_buckets, river_ip_buckets,
                        oop_scores, ip_scores, valid_ip_for_oop, valid_oop_for_ip,
                        river_scale, river_oop_cfr, river_ip_cfr, strat_buf, is_br,
                    )
                }
            }
        }
        TreeNode::Action {
            node_id,
            player,
            children,
            actions,
            ..
        } => {
            let num_actions = actions.len();
            let nid = *node_id as usize;

            if *player == br_player {
                if is_br {
                    let mut best = f64::NEG_INFINITY;
                    for a in 0..num_actions {
                        let v = br_traverse_turn_template(
                            &children[a], br_player, hand_idx, turn_bucket, river_bucket,
                            opp_reach, oop_combos, ip_combos,
                            turn_oop_buckets, turn_ip_buckets,
                            river_oop_buckets, river_ip_buckets,
                            oop_scores, ip_scores, valid_ip_for_oop, valid_oop_for_ip,
                            scale, river_template, turn_oop_cfr, turn_ip_cfr,
                            river_oop_cfr, river_ip_cfr, strat_buf, is_br,
                        );
                        if v > best { best = v; }
                    }
                    best
                } else {
                    let cfr = match br_player {
                        Player::OOP => turn_oop_cfr,
                        Player::IP => turn_ip_cfr,
                    };
                    cfr.average_strategy(nid, turn_bucket, strat_buf);
                    let mut nv = 0.0;
                    for a in 0..num_actions {
                        let v = br_traverse_turn_template(
                            &children[a], br_player, hand_idx, turn_bucket, river_bucket,
                            opp_reach, oop_combos, ip_combos,
                            turn_oop_buckets, turn_ip_buckets,
                            river_oop_buckets, river_ip_buckets,
                            oop_scores, ip_scores, valid_ip_for_oop, valid_oop_for_ip,
                            scale, river_template, turn_oop_cfr, turn_ip_cfr,
                            river_oop_cfr, river_ip_cfr, strat_buf, is_br,
                        );
                        nv += strat_buf[a] as f64 * v;
                    }
                    nv
                }
            } else {
                let opp_cfr = match br_player {
                    Player::OOP => turn_ip_cfr,
                    Player::IP => turn_oop_cfr,
                };
                let opp_buckets = match br_player {
                    Player::OOP => turn_ip_buckets,
                    Player::IP => turn_oop_buckets,
                };
                let num_opp = opp_reach.len();
                let mut nv = 0.0;
                for a in 0..num_actions {
                    let mut new_opp_reach = vec![0.0f64; num_opp];
                    for j in 0..num_opp {
                        if opp_reach[j] > 0.0 {
                            let b = opp_buckets[j] as usize;
                            opp_cfr.average_strategy(nid, b, strat_buf);
                            new_opp_reach[j] = opp_reach[j] * strat_buf[a] as f64;
                        }
                    }
                    nv += br_traverse_turn_template(
                        &children[a], br_player, hand_idx, turn_bucket, river_bucket,
                        &new_opp_reach, oop_combos, ip_combos,
                        turn_oop_buckets, turn_ip_buckets,
                        river_oop_buckets, river_ip_buckets,
                        oop_scores, ip_scores, valid_ip_for_oop, valid_oop_for_ip,
                        scale, river_template, turn_oop_cfr, turn_ip_cfr,
                        river_oop_cfr, river_ip_cfr, strat_buf, is_br,
                    );
                }
                nv
            }
        }
        TreeNode::Chance { .. } => unreachable!(),
    }
}

#[allow(clippy::too_many_arguments)]
fn br_traverse_river_template(
    node: &TreeNode,
    br_player: Player,
    hand_idx: usize,
    river_bucket: usize,
    opp_reach: &[f64],
    oop_combos: &[Combo],
    ip_combos: &[Combo],
    river_oop_buckets: &[u16],
    river_ip_buckets: &[u16],
    oop_scores: &[u32],
    ip_scores: &[u32],
    valid_ip_for_oop: &[Vec<u16>],
    valid_oop_for_ip: &[Vec<u16>],
    scale: f64,
    river_oop_cfr: &FlatCfr,
    river_ip_cfr: &FlatCfr,
    strat_buf: &mut [f32],
    is_br: bool,
) -> f64 {
    match node {
        TreeNode::Terminal {
            terminal_type,
            pot,
            invested,
            ..
        } => {
            let opp_reach_sum: f64 = opp_reach.iter().sum();
            if opp_reach_sum < 1e-10 {
                return 0.0;
            }
            match terminal_type {
                TerminalType::Fold { folder } => {
                    let my_invested = invested[br_player.index()] * scale;
                    if *folder == br_player {
                        -my_invested * opp_reach_sum
                    } else {
                        (*pot * scale - my_invested) * opp_reach_sum
                    }
                }
                TerminalType::Showdown => {
                    let pot_s = *pot * scale;
                    let my_inv = invested[br_player.index()] * scale;
                    let win = pot_s - my_inv;
                    let lose = -my_inv;
                    let tie = pot_s / 2.0 - my_inv;
                    let mut value = 0.0;
                    match br_player {
                        Player::OOP => {
                            let ms = oop_scores[hand_idx];
                            for &j in &valid_ip_for_oop[hand_idx] {
                                let j = j as usize;
                                if opp_reach[j] < 1e-10 { continue; }
                                let os = ip_scores[j];
                                value += opp_reach[j] * if ms > os { win } else if ms < os { lose } else { tie };
                            }
                        }
                        Player::IP => {
                            let ms = ip_scores[hand_idx];
                            for &i in &valid_oop_for_ip[hand_idx] {
                                let i = i as usize;
                                if opp_reach[i] < 1e-10 { continue; }
                                let os = oop_scores[i];
                                value += opp_reach[i] * if ms > os { win } else if ms < os { lose } else { tie };
                            }
                        }
                    }
                    value
                }
            }
        }
        TreeNode::Action {
            node_id,
            player,
            children,
            actions,
            ..
        } => {
            let num_actions = actions.len();
            let nid = *node_id as usize;

            if *player == br_player {
                if is_br {
                    let mut best = f64::NEG_INFINITY;
                    for a in 0..num_actions {
                        let v = br_traverse_river_template(
                            &children[a], br_player, hand_idx, river_bucket,
                            opp_reach, oop_combos, ip_combos,
                            river_oop_buckets, river_ip_buckets,
                            oop_scores, ip_scores, valid_ip_for_oop, valid_oop_for_ip,
                            scale, river_oop_cfr, river_ip_cfr, strat_buf, is_br,
                        );
                        if v > best { best = v; }
                    }
                    best
                } else {
                    let cfr = match br_player {
                        Player::OOP => river_oop_cfr,
                        Player::IP => river_ip_cfr,
                    };
                    cfr.average_strategy(nid, river_bucket, strat_buf);
                    let mut nv = 0.0;
                    for a in 0..num_actions {
                        let v = br_traverse_river_template(
                            &children[a], br_player, hand_idx, river_bucket,
                            opp_reach, oop_combos, ip_combos,
                            river_oop_buckets, river_ip_buckets,
                            oop_scores, ip_scores, valid_ip_for_oop, valid_oop_for_ip,
                            scale, river_oop_cfr, river_ip_cfr, strat_buf, is_br,
                        );
                        nv += strat_buf[a] as f64 * v;
                    }
                    nv
                }
            } else {
                let opp_cfr = match br_player {
                    Player::OOP => river_ip_cfr,
                    Player::IP => river_oop_cfr,
                };
                let opp_buckets = match br_player {
                    Player::OOP => river_ip_buckets,
                    Player::IP => river_oop_buckets,
                };
                let num_opp = opp_reach.len();
                let mut nv = 0.0;
                for a in 0..num_actions {
                    let mut new_opp_reach = vec![0.0f64; num_opp];
                    for j in 0..num_opp {
                        if opp_reach[j] > 0.0 {
                            let b = opp_buckets[j] as usize;
                            opp_cfr.average_strategy(nid, b, strat_buf);
                            new_opp_reach[j] = opp_reach[j] * strat_buf[a] as f64;
                        }
                    }
                    nv += br_traverse_river_template(
                        &children[a], br_player, hand_idx, river_bucket,
                        &new_opp_reach, oop_combos, ip_combos,
                        river_oop_buckets, river_ip_buckets,
                        oop_scores, ip_scores, valid_ip_for_oop, valid_oop_for_ip,
                        scale, river_oop_cfr, river_ip_cfr, strat_buf, is_br,
                    );
                }
                nv
            }
        }
        TreeNode::Chance { .. } => unreachable!(),
    }
}

// ---------------------------------------------------------------------------
// Solution extraction
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
fn extract_solution(
    config: &FlopSolverConfig,
    flop_tree: &TreeNode,
    flop_oop_cfr: &FlatCfr,
    flop_ip_cfr: &FlatCfr,
    oop_combos: &[Combo],
    ip_combos: &[Combo],
    flop_oop_buckets: &[u16],
    flop_ip_buckets: &[u16],
    _metas: &[crate::postflop_tree::NodeMeta],
    turn_template: &TreeNode,
    river_template: &TreeNode,
    turn_oop_cfr: &FlatCfr,
    turn_ip_cfr: &FlatCfr,
    river_oop_cfr: &FlatCfr,
    river_ip_cfr: &FlatCfr,
    oop_blockers: &[[bool; 52]],
    ip_blockers: &[[bool; 52]],
    valid_ip_for_oop: &[Vec<u16>],
    valid_oop_for_ip: &[Vec<u16>],
) -> FlopSolution {
    // Compute exploitability
    let exploitability = estimate_exploitability(
        flop_tree,
        turn_template,
        river_template,
        flop_oop_cfr,
        flop_ip_cfr,
        turn_oop_cfr,
        turn_ip_cfr,
        river_oop_cfr,
        river_ip_cfr,
        oop_combos,
        ip_combos,
        oop_blockers,
        ip_blockers,
        flop_oop_buckets,
        flop_ip_buckets,
        valid_ip_for_oop,
        valid_oop_for_ip,
        &config.board,
        config.starting_pot,
        config.num_buckets,
    );

    // Extract flop-level strategies (combo-level from bucket-level)
    let mut strategies = Vec::new();
    extract_flop_strategies(
        flop_tree,
        flop_oop_cfr,
        flop_ip_cfr,
        oop_combos,
        ip_combos,
        flop_oop_buckets,
        flop_ip_buckets,
        &mut strategies,
    );

    // Extract turn/river template strategies at bucket level (zero extra compute)
    let mut turn_strategies = Vec::new();
    extract_template_strategies(
        turn_template,
        turn_oop_cfr,
        turn_ip_cfr,
        config.num_buckets,
        &mut turn_strategies,
    );
    let mut river_strategies = Vec::new();
    extract_template_strategies(
        river_template,
        river_oop_cfr,
        river_ip_cfr,
        config.num_buckets,
        &mut river_strategies,
    );

    let board_str = config
        .board
        .iter()
        .map(|&b| format!("{}", index_to_card(b)))
        .collect::<String>();

    let oop_combo_strs: Vec<String> = oop_combos
        .iter()
        .map(|c| format!("{}{}", index_to_card(c.0), index_to_card(c.1)))
        .collect();
    let ip_combo_strs: Vec<String> = ip_combos
        .iter()
        .map(|c| format!("{}{}", index_to_card(c.0), index_to_card(c.1)))
        .collect();

    FlopSolution {
        board: board_str,
        oop_range: config.oop_range.clone(),
        ip_range: config.ip_range.clone(),
        starting_pot: config.starting_pot,
        effective_stack: config.effective_stack,
        iterations: config.iterations,
        exploitability,
        oop_combos: oop_combo_strs,
        ip_combos: ip_combo_strs,
        strategies,
        oop_pos: String::new(),
        ip_pos: String::new(),
        turn_strategies,
        river_strategies,
        num_buckets: config.num_buckets,
    }
}

fn extract_flop_strategies(
    node: &TreeNode,
    flop_oop_cfr: &FlatCfr,
    flop_ip_cfr: &FlatCfr,
    oop_combos: &[Combo],
    ip_combos: &[Combo],
    flop_oop_buckets: &[u16],
    flop_ip_buckets: &[u16],
    strategies: &mut Vec<FlopNodeStrategy>,
) {
    match node {
        TreeNode::Action {
            node_id,
            player,
            children,
            actions,
            ..
        } => {
            let num_actions = actions.len();
            let nid = *node_id as usize;
            let (cfr, num_combos, buckets) = match player {
                Player::OOP => (flop_oop_cfr, oop_combos.len(), flop_oop_buckets),
                Player::IP => (flop_ip_cfr, ip_combos.len(), flop_ip_buckets),
            };

            let mut avg_buf = vec![0.0f32; num_actions];
            let frequencies: Vec<Vec<f64>> = (0..num_combos)
                .map(|h| {
                    let bucket = buckets[h] as usize;
                    cfr.average_strategy(nid, bucket, &mut avg_buf);
                    avg_buf[..num_actions].iter().map(|&v| v as f64).collect()
                })
                .collect();

            strategies.push(FlopNodeStrategy {
                node_id: *node_id,
                player: match player {
                    Player::OOP => "OOP".to_string(),
                    Player::IP => "IP".to_string(),
                },
                actions: actions.iter().map(|a| a.label()).collect(),
                frequencies,
            });

            for child in children {
                extract_flop_strategies(
                    child,
                    flop_oop_cfr,
                    flop_ip_cfr,
                    oop_combos,
                    ip_combos,
                    flop_oop_buckets,
                    flop_ip_buckets,
                    strategies,
                );
            }
        }
        TreeNode::Terminal { .. } => {}
        TreeNode::Chance { .. } => {}
    }
}

/// Extract bucket-level strategies from a template tree (turn or river).
///
/// Walks the tree and for each action node, reads the average strategy from
/// the corresponding FlatCfr at each bucket index.
fn extract_template_strategies(
    node: &TreeNode,
    oop_cfr: &FlatCfr,
    ip_cfr: &FlatCfr,
    num_buckets: usize,
    strategies: &mut Vec<TemplateBucketStrategy>,
) {
    match node {
        TreeNode::Action {
            node_id,
            player,
            children,
            actions,
            ..
        } => {
            let num_actions = actions.len();
            let nid = *node_id as usize;
            let cfr = match player {
                Player::OOP => oop_cfr,
                Player::IP => ip_cfr,
            };

            let mut avg_buf = vec![0.0f32; num_actions];
            let frequencies: Vec<Vec<f64>> = (0..num_buckets)
                .map(|b| {
                    cfr.average_strategy(nid, b, &mut avg_buf);
                    avg_buf[..num_actions].iter().map(|&v| v as f64).collect()
                })
                .collect();

            strategies.push(TemplateBucketStrategy {
                node_id: *node_id,
                player: match player {
                    Player::OOP => "OOP".to_string(),
                    Player::IP => "IP".to_string(),
                },
                actions: actions.iter().map(|a| a.label()).collect(),
                frequencies,
            });

            for child in children {
                extract_template_strategies(child, oop_cfr, ip_cfr, num_buckets, strategies);
            }
        }
        TreeNode::Terminal { .. } => {}
        TreeNode::Chance { .. } => {}
    }
}

fn empty_solution(config: &FlopSolverConfig) -> FlopSolution {
    let board_str = config
        .board
        .iter()
        .map(|&b| format!("{}", index_to_card(b)))
        .collect::<String>();

    FlopSolution {
        board: board_str,
        oop_range: config.oop_range.clone(),
        ip_range: config.ip_range.clone(),
        starting_pot: config.starting_pot,
        effective_stack: config.effective_stack,
        iterations: config.iterations,
        exploitability: 0.0,
        oop_combos: vec![],
        ip_combos: vec![],
        strategies: vec![],
        oop_pos: String::new(),
        ip_pos: String::new(),
        turn_strategies: vec![],
        river_strategies: vec![],
        num_buckets: 0,
    }
}

// ---------------------------------------------------------------------------
// Display
// ---------------------------------------------------------------------------

impl FlopSolution {
    pub fn display(&self) {
        use colored::Colorize;

        println!();
        println!(
            "  {} Flop Solution  |  Board: {}  |  Pot: {:.0}  |  Stack: {:.0}  |  {} iterations",
            "GTO".bold(),
            self.board,
            self.starting_pot,
            self.effective_stack,
            self.iterations,
        );
        println!("  Exploitability: {:.4}", self.exploitability);
        println!(
            "  OOP range: {} ({} combos)  |  IP range: {} ({} combos)",
            self.oop_range.join(","),
            self.oop_combos.len(),
            self.ip_range.join(","),
            self.ip_combos.len(),
        );

        if let Some(root_strat) = self.strategies.first() {
            println!();
            println!(
                "  {} at root (node {}):",
                root_strat.player.bold(),
                root_strat.node_id
            );
            println!("  Actions: {}", root_strat.actions.join(" | "));

            let num_to_show = root_strat.frequencies.len().min(20);
            let combos = if root_strat.player == "OOP" {
                &self.oop_combos
            } else {
                &self.ip_combos
            };

            for i in 0..num_to_show {
                let freq_str: String = root_strat.frequencies[i]
                    .iter()
                    .zip(&root_strat.actions)
                    .map(|(f, a)| {
                        let pct = (f * 100.0).round() as u32;
                        if pct > 70 {
                            format!("{}:{}", a, format!("{}%", pct).green())
                        } else if pct > 30 {
                            format!("{}:{}", a, format!("{}%", pct).yellow())
                        } else {
                            format!("{}:{}%", a, pct)
                        }
                    })
                    .collect::<Vec<_>>()
                    .join("  ");
                println!("    {}  {}", combos[i].bold(), freq_str);
            }
            if root_strat.frequencies.len() > num_to_show {
                println!(
                    "    ... and {} more combos",
                    root_strat.frequencies.len() - num_to_show
                );
            }
        }

        println!();
    }
}

// ---------------------------------------------------------------------------
// Cache
// ---------------------------------------------------------------------------

impl FlopSolution {
    pub fn cache_path(&self) -> std::path::PathBuf {
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
        let dir = std::path::Path::new(&home).join(".gto-cli").join("solver");
        std::fs::create_dir_all(&dir).ok();
        dir.join(format!(
            "flop_{}_{}_{}_{:.0}_{:.0}.bin",
            self.board, self.oop_pos, self.ip_pos, self.starting_pot, self.effective_stack,
        ))
    }

    pub fn save_cache(&self) {
        if let Ok(data) = bincode::serialize(self) {
            let path = self.cache_path();
            std::fs::write(path, data).ok();
        }
    }

    pub fn load_cache(board: &str, oop_pos: &str, ip_pos: &str, pot: f64, stack: f64) -> Option<FlopSolution> {
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
        let path = std::path::Path::new(&home)
            .join(".gto-cli")
            .join("solver")
            .join(format!("flop_{}_{}_{}_{:.0}_{:.0}.bin", board, oop_pos, ip_pos, pot, stack));
        let data = std::fs::read(path).ok()?;
        bincode::deserialize(&data).ok()
    }
}
