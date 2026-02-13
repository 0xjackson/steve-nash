//! Turn CFR+ solver.
//!
//! Solves heads-up turn spots using CFR+ over a turn+river game tree.
//! At chance nodes (river card dealt), hand strengths are re-evaluated
//! and blocker-aware reach probabilities are updated.
//!
//! Uses `FlatCfr` for memory-efficient storage (~5x vs HashMap-based)
//! and two separate instances (one per player) to avoid borrow conflicts.

use serde::{Deserialize, Serialize};

use crate::card_encoding::{card_to_index, index_to_card};
use crate::cards::parse_board;
use crate::flat_cfr::FlatCfr;
use crate::lookup_eval::evaluate_fast;
use crate::postflop_tree::{
    build_turn_tree, collect_node_metadata, Player, TerminalType, TreeNode, TurnTreeConfig,
};
use crate::ranges::parse_range;
use crate::river_solver::{expand_range_to_combos, Combo};

// ---------------------------------------------------------------------------
// Config & result
// ---------------------------------------------------------------------------

pub struct TurnSolverConfig {
    /// 4-card turn board as u8 indices.
    pub board: Vec<u8>,
    pub oop_range: Vec<String>,
    pub ip_range: Vec<String>,
    pub starting_pot: f64,
    pub effective_stack: f64,
    pub iterations: usize,
}

impl TurnSolverConfig {
    pub fn new(
        board_str: &str,
        oop_range_str: &str,
        ip_range_str: &str,
        starting_pot: f64,
        effective_stack: f64,
        iterations: usize,
    ) -> Result<Self, String> {
        let board_cards = parse_board(board_str).map_err(|e| e.to_string())?;
        if board_cards.len() != 4 {
            return Err("Turn board must have exactly 4 cards".to_string());
        }
        let board: Vec<u8> = board_cards.iter().map(|c| card_to_index(c)).collect();
        let oop_range = parse_range(oop_range_str);
        let ip_range = parse_range(ip_range_str);

        if oop_range.is_empty() {
            return Err("OOP range is empty".to_string());
        }
        if ip_range.is_empty() {
            return Err("IP range is empty".to_string());
        }

        Ok(TurnSolverConfig {
            board,
            oop_range,
            ip_range,
            starting_pot,
            effective_stack,
            iterations,
        })
    }
}

/// Per-node strategy for the turn solution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TurnNodeStrategy {
    pub node_id: u16,
    pub player: String,
    pub actions: Vec<String>,
    /// Average strategy frequencies: [combo_idx][action_idx].
    pub frequencies: Vec<Vec<f64>>,
}

/// Full solution from the turn solver.
#[derive(Debug, Serialize, Deserialize)]
pub struct TurnSolution {
    pub board: String,
    pub oop_range: Vec<String>,
    pub ip_range: Vec<String>,
    pub starting_pot: f64,
    pub effective_stack: f64,
    pub iterations: usize,
    pub exploitability: f64,
    pub oop_combos: Vec<String>,
    pub ip_combos: Vec<String>,
    /// Strategies for turn-level action nodes only (root + turn betting).
    pub strategies: Vec<TurnNodeStrategy>,
}

// ---------------------------------------------------------------------------
// Solver
// ---------------------------------------------------------------------------

/// Solve a turn spot. Returns the full solution including exploitability.
pub fn solve_turn(config: &TurnSolverConfig) -> TurnSolution {
    let tree_config = TurnTreeConfig::new(
        config.board.clone(),
        config.starting_pot,
        config.effective_stack,
    );
    let (tree, _num_nodes) = build_turn_tree(&tree_config);

    let oop_combos = expand_range_to_combos(&config.oop_range, &config.board);
    let ip_combos = expand_range_to_combos(&config.ip_range, &config.board);

    if oop_combos.is_empty() || ip_combos.is_empty() {
        return empty_solution(config);
    }

    // Collect node metadata and build FlatCfr instances per player
    let metas = collect_node_metadata(&tree);
    let num_oop = oop_combos.len() as u16;
    let num_ip = ip_combos.len() as u16;

    let oop_nodes: Vec<(u8, u16)> = metas
        .iter()
        .map(|m| {
            let hands = if m.player == Player::OOP { num_oop } else { 0 };
            (m.num_actions, hands)
        })
        .collect();
    let ip_nodes: Vec<(u8, u16)> = metas
        .iter()
        .map(|m| {
            let hands = if m.player == Player::IP { num_ip } else { 0 };
            (m.num_actions, hands)
        })
        .collect();

    let mut oop_cfr = FlatCfr::new(&oop_nodes);
    let mut ip_cfr = FlatCfr::new(&ip_nodes);

    // Precompute: blocker sets for each combo
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

    // Precompute: validity tables (which OOP combos are valid for each IP combo and vice versa)
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
    let max_actions = metas.iter().map(|m| m.num_actions).max().unwrap_or(1) as usize;
    let mut strategy_buf = vec![0.0f32; max_actions];
    let mut action_values = vec![0.0f32; max_actions];

    // Run alternating CFR+ iterations
    for iter in 0..config.iterations {
        let traverser = if iter % 2 == 0 { Player::OOP } else { Player::IP };

        let num_combos = match traverser {
            Player::OOP => oop_combos.len(),
            Player::IP => ip_combos.len(),
        };

        for h in 0..num_combos {
            // Initialize opponent reach: 1.0 for non-conflicting, 0.0 for blocked
            let opp_reach = match traverser {
                Player::OOP => {
                    let valid = &valid_ip_for_oop[h];
                    let mut reach = vec![0.0f64; ip_combos.len()];
                    for &j in valid {
                        reach[j as usize] = 1.0;
                    }
                    reach
                }
                Player::IP => {
                    let valid = &valid_oop_for_ip[h];
                    let mut reach = vec![0.0f64; oop_combos.len()];
                    for &i in valid {
                        reach[i as usize] = 1.0;
                    }
                    reach
                }
            };

            cfr_traverse_turn(
                &tree,
                traverser,
                h,
                &opp_reach,
                &oop_combos,
                &ip_combos,
                &oop_blockers,
                &ip_blockers,
                &config.board,
                &mut oop_cfr,
                &mut ip_cfr,
                &mut strategy_buf,
                &mut action_values,
                iter,
            );
        }
    }

    // Extract solution
    extract_solution(
        config,
        &tree,
        &oop_cfr,
        &ip_cfr,
        &oop_combos,
        &ip_combos,
        &metas,
    )
}

// ---------------------------------------------------------------------------
// CFR+ traversal
// ---------------------------------------------------------------------------

/// Recursive CFR+ traversal for river subtrees (inside chance nodes).
/// `river_board` is the full 5-card board (turn board + dealt river card).
#[allow(clippy::too_many_arguments)]
fn cfr_traverse_river(
    node: &TreeNode,
    traverser: Player,
    hand_idx: usize,
    opp_reach: &[f64],
    oop_combos: &[Combo],
    ip_combos: &[Combo],
    river_board: &[u8; 5],
    oop_scores: &[u32],
    ip_scores: &[u32],
    valid_ip_for_oop_h: &[u16],
    valid_oop_for_ip_h: &[u16],
    oop_cfr: &mut FlatCfr,
    ip_cfr: &mut FlatCfr,
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
                    let win_payoff = *pot - my_invested;
                    let lose_payoff = -my_invested;
                    let tie_payoff = *pot / 2.0 - my_invested;
                    let mut value = 0.0;

                    match traverser {
                        Player::OOP => {
                            let my_score = oop_scores[hand_idx];
                            for &j in valid_ip_for_oop_h {
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
                            for &i in valid_oop_for_ip_h {
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
                    Player::OOP => &*oop_cfr,
                    Player::IP => &*ip_cfr,
                };
                cfr.current_strategy(nid, hand_idx, strategy_buf);

                let mut node_value = 0.0f64;
                for a in 0..num_actions {
                    // Regret pruning: skip near-zero-probability actions after warmup
                    if strategy_buf[a] < 0.001 && iter > 1000 && iter % 1000 != 0 {
                        action_values_buf[a] = 0.0;
                        continue;
                    }
                    let av = cfr_traverse_river(
                        &children[a],
                        traverser,
                        hand_idx,
                        opp_reach,
                        oop_combos,
                        ip_combos,
                        river_board,
                        oop_scores,
                        ip_scores,
                        valid_ip_for_oop_h,
                        valid_oop_for_ip_h,
                        oop_cfr,
                        ip_cfr,
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
                    Player::OOP => &mut *oop_cfr,
                    Player::IP => &mut *ip_cfr,
                };
                cfr_mut.update(
                    nid,
                    hand_idx,
                    &action_values_buf[..num_actions],
                    node_value as f32,
                    reach_prob,
                );

                node_value
            } else {
                let opp_cfr = match traverser {
                    Player::OOP => &*ip_cfr,
                    Player::IP => &*oop_cfr,
                };
                let num_opp = opp_reach.len();
                let opp_num_actions = opp_cfr.node_num_actions(nid) as usize;

                let mut opp_strats = vec![0.0f32; num_opp * opp_num_actions];
                for j in 0..num_opp {
                    if opp_reach[j] > 0.0 {
                        opp_cfr.current_strategy(
                            nid,
                            j,
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

                    node_value += cfr_traverse_river(
                        &children[a],
                        traverser,
                        hand_idx,
                        &new_opp_reach,
                        oop_combos,
                        ip_combos,
                        river_board,
                        oop_scores,
                        ip_scores,
                        valid_ip_for_oop_h,
                        valid_oop_for_ip_h,
                        oop_cfr,
                        ip_cfr,
                        strategy_buf,
                        action_values_buf,
                        iter,
                    );
                }

                node_value
            }
        }
        TreeNode::Chance { .. } => {
            unreachable!("River subtree should not contain chance nodes")
        }
    }
}

// ---------------------------------------------------------------------------
// Updated top-level CFR traversal with proper chance handling
// ---------------------------------------------------------------------------

/// Top-level CFR+ traversal for the turn tree.
/// Handles turn action nodes and chance nodes (delegates to river traversal).
#[allow(clippy::too_many_arguments)]
fn cfr_traverse_turn(
    node: &TreeNode,
    traverser: Player,
    hand_idx: usize,
    opp_reach: &[f64],
    oop_combos: &[Combo],
    ip_combos: &[Combo],
    oop_blockers: &[[bool; 52]],
    ip_blockers: &[[bool; 52]],
    board: &[u8],
    oop_cfr: &mut FlatCfr,
    ip_cfr: &mut FlatCfr,
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
            // Fold terminals at turn level
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
                    // Shouldn't happen at turn level (all converted to Chance)
                    0.0
                }
            }
        }
        TreeNode::Chance {
            cards, children, ..
        } => {
            let mut total_value = 0.0;
            let mut valid_count = 0usize;

            for (ci, &river_card) in cards.iter().enumerate() {
                // Skip if traverser's hand blocks this river card
                let traverser_blocked = match traverser {
                    Player::OOP => oop_blockers[hand_idx][river_card as usize],
                    Player::IP => ip_blockers[hand_idx][river_card as usize],
                };
                if traverser_blocked {
                    continue;
                }
                valid_count += 1;

                // Build new opp_reach: zero out opponents blocked by river card
                let new_opp_reach: Vec<f64> = match traverser {
                    Player::OOP => opp_reach
                        .iter()
                        .enumerate()
                        .map(|(j, &r)| {
                            if r > 0.0 && !ip_blockers[j][river_card as usize] {
                                r
                            } else {
                                0.0
                            }
                        })
                        .collect(),
                    Player::IP => opp_reach
                        .iter()
                        .enumerate()
                        .map(|(i, &r)| {
                            if r > 0.0 && !oop_blockers[i][river_card as usize] {
                                r
                            } else {
                                0.0
                            }
                        })
                        .collect(),
                };

                // Build 5-card river board
                let river_board: [u8; 5] = [board[0], board[1], board[2], board[3], river_card];

                // Evaluate hand strengths for this river card
                let oop_scores: Vec<u32> = oop_combos
                    .iter()
                    .map(|c| {
                        evaluate_fast(&[
                            c.0,
                            c.1,
                            river_board[0],
                            river_board[1],
                            river_board[2],
                            river_board[3],
                            river_board[4],
                        ])
                    })
                    .collect();
                let ip_scores: Vec<u32> = ip_combos
                    .iter()
                    .map(|c| {
                        evaluate_fast(&[
                            c.0,
                            c.1,
                            river_board[0],
                            river_board[1],
                            river_board[2],
                            river_board[3],
                            river_board[4],
                        ])
                    })
                    .collect();

                // Validity tables for this hand against opponents (blocker-aware)
                let (valid_ip_h, valid_oop_h) = match traverser {
                    Player::OOP => {
                        let valid_ip: Vec<u16> = ip_combos
                            .iter()
                            .enumerate()
                            .filter(|(_, ip)| {
                                let oop = &oop_combos[hand_idx];
                                oop.0 != ip.0
                                    && oop.0 != ip.1
                                    && oop.1 != ip.0
                                    && oop.1 != ip.1
                                    && ip.0 != river_card
                                    && ip.1 != river_card
                            })
                            .map(|(j, _)| j as u16)
                            .collect();
                        (valid_ip, Vec::new())
                    }
                    Player::IP => {
                        let valid_oop: Vec<u16> = oop_combos
                            .iter()
                            .enumerate()
                            .filter(|(_, oop)| {
                                let ip = &ip_combos[hand_idx];
                                ip.0 != oop.0
                                    && ip.0 != oop.1
                                    && ip.1 != oop.0
                                    && ip.1 != oop.1
                                    && oop.0 != river_card
                                    && oop.1 != river_card
                            })
                            .map(|(i, _)| i as u16)
                            .collect();
                        (Vec::new(), valid_oop)
                    }
                };

                let child_value = cfr_traverse_river(
                    &children[ci],
                    traverser,
                    hand_idx,
                    &new_opp_reach,
                    oop_combos,
                    ip_combos,
                    &river_board,
                    &oop_scores,
                    &ip_scores,
                    &valid_ip_h,
                    &valid_oop_h,
                    oop_cfr,
                    ip_cfr,
                    strategy_buf,
                    action_values_buf,
                    iter,
                );
                total_value += child_value;
            }

            if valid_count > 0 {
                total_value / valid_count as f64
            } else {
                0.0
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
                    Player::OOP => &*oop_cfr,
                    Player::IP => &*ip_cfr,
                };
                cfr.current_strategy(nid, hand_idx, strategy_buf);

                let mut node_value = 0.0f64;
                for a in 0..num_actions {
                    // Regret pruning: skip near-zero-probability actions after warmup
                    if strategy_buf[a] < 0.001 && iter > 1000 && iter % 1000 != 0 {
                        action_values_buf[a] = 0.0;
                        continue;
                    }
                    let av = cfr_traverse_turn(
                        &children[a],
                        traverser,
                        hand_idx,
                        opp_reach,
                        oop_combos,
                        ip_combos,
                        oop_blockers,
                        ip_blockers,
                        board,
                        oop_cfr,
                        ip_cfr,
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
                    Player::OOP => &mut *oop_cfr,
                    Player::IP => &mut *ip_cfr,
                };
                cfr_mut.update(
                    nid,
                    hand_idx,
                    &action_values_buf[..num_actions],
                    node_value as f32,
                    reach_prob,
                );

                node_value
            } else {
                let opp_cfr = match traverser {
                    Player::OOP => &*ip_cfr,
                    Player::IP => &*oop_cfr,
                };
                let num_opp = opp_reach.len();
                let opp_num_actions = opp_cfr.node_num_actions(nid) as usize;

                let mut opp_strats = vec![0.0f32; num_opp * opp_num_actions];
                for j in 0..num_opp {
                    if opp_reach[j] > 0.0 {
                        opp_cfr.current_strategy(
                            nid,
                            j,
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

                    node_value += cfr_traverse_turn(
                        &children[a],
                        traverser,
                        hand_idx,
                        &new_opp_reach,
                        oop_combos,
                        ip_combos,
                        oop_blockers,
                        ip_blockers,
                        board,
                        oop_cfr,
                        ip_cfr,
                        strategy_buf,
                        action_values_buf,
                        iter,
                    );
                }

                node_value
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Exploitability
// ---------------------------------------------------------------------------

/// Compute exploitability via best-response traversal.
pub fn compute_exploitability(
    tree: &TreeNode,
    oop_cfr: &FlatCfr,
    ip_cfr: &FlatCfr,
    oop_combos: &[Combo],
    ip_combos: &[Combo],
    oop_blockers: &[[bool; 52]],
    ip_blockers: &[[bool; 52]],
    board: &[u8],
) -> f64 {
    let oop_gain = best_response_value(
        tree,
        Player::OOP,
        oop_cfr,
        ip_cfr,
        oop_combos,
        ip_combos,
        oop_blockers,
        ip_blockers,
        board,
    );
    let ip_gain = best_response_value(
        tree,
        Player::IP,
        oop_cfr,
        ip_cfr,
        oop_combos,
        ip_combos,
        oop_blockers,
        ip_blockers,
        board,
    );
    (oop_gain + ip_gain) / 2.0
}

#[allow(clippy::too_many_arguments)]
fn best_response_value(
    tree: &TreeNode,
    br_player: Player,
    oop_cfr: &FlatCfr,
    ip_cfr: &FlatCfr,
    oop_combos: &[Combo],
    ip_combos: &[Combo],
    oop_blockers: &[[bool; 52]],
    ip_blockers: &[[bool; 52]],
    board: &[u8],
) -> f64 {
    let num_br = match br_player {
        Player::OOP => oop_combos.len(),
        Player::IP => ip_combos.len(),
    };
    let num_opp = match br_player {
        Player::OOP => ip_combos.len(),
        Player::IP => oop_combos.len(),
    };

    let valid_for: Vec<Vec<u16>> = (0..num_br)
        .map(|h| match br_player {
            Player::OOP => ip_combos
                .iter()
                .enumerate()
                .filter(|(_, ip)| {
                    let oop = &oop_combos[h];
                    oop.0 != ip.0 && oop.0 != ip.1 && oop.1 != ip.0 && oop.1 != ip.1
                })
                .map(|(j, _)| j as u16)
                .collect(),
            Player::IP => oop_combos
                .iter()
                .enumerate()
                .filter(|(_, oop)| {
                    let ip = &ip_combos[h];
                    ip.0 != oop.0 && ip.0 != oop.1 && ip.1 != oop.0 && ip.1 != oop.1
                })
                .map(|(i, _)| i as u16)
                .collect(),
        })
        .collect();

    let mut total_gain = 0.0;
    let mut strat_buf = vec![0.0f32; 16]; // max actions at any node

    for h in 0..num_br {
        let mut opp_reach = vec![0.0f64; num_opp];
        for &j in &valid_for[h] {
            opp_reach[j as usize] = 1.0;
        }

        let br_value = br_traverse_turn(
            tree,
            br_player,
            h,
            &opp_reach,
            oop_combos,
            ip_combos,
            oop_blockers,
            ip_blockers,
            board,
            oop_cfr,
            ip_cfr,
            &mut strat_buf,
            true, // best response
        );

        let avg_value = br_traverse_turn(
            tree,
            br_player,
            h,
            &opp_reach,
            oop_combos,
            ip_combos,
            oop_blockers,
            ip_blockers,
            board,
            oop_cfr,
            ip_cfr,
            &mut strat_buf,
            false, // average strategy
        );

        total_gain += br_value - avg_value;
    }

    total_gain / num_br as f64
}

/// Best-response / average-strategy traversal for exploitability.
#[allow(clippy::too_many_arguments)]
fn br_traverse_turn(
    node: &TreeNode,
    br_player: Player,
    hand_idx: usize,
    opp_reach: &[f64],
    oop_combos: &[Combo],
    ip_combos: &[Combo],
    oop_blockers: &[[bool; 52]],
    ip_blockers: &[[bool; 52]],
    board: &[u8],
    oop_cfr: &FlatCfr,
    ip_cfr: &FlatCfr,
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
                TerminalType::Showdown => 0.0, // Turn-level showdown shouldn't exist
            }
        }
        TreeNode::Chance {
            cards, children, ..
        } => {
            let mut total_value = 0.0;
            let mut valid_count = 0usize;

            for (ci, &river_card) in cards.iter().enumerate() {
                let blocked = match br_player {
                    Player::OOP => oop_blockers[hand_idx][river_card as usize],
                    Player::IP => ip_blockers[hand_idx][river_card as usize],
                };
                if blocked {
                    continue;
                }
                valid_count += 1;

                let new_opp_reach: Vec<f64> = match br_player {
                    Player::OOP => opp_reach
                        .iter()
                        .enumerate()
                        .map(|(j, &r)| {
                            if r > 0.0 && !ip_blockers[j][river_card as usize] {
                                r
                            } else {
                                0.0
                            }
                        })
                        .collect(),
                    Player::IP => opp_reach
                        .iter()
                        .enumerate()
                        .map(|(i, &r)| {
                            if r > 0.0 && !oop_blockers[i][river_card as usize] {
                                r
                            } else {
                                0.0
                            }
                        })
                        .collect(),
                };

                let river_board = [board[0], board[1], board[2], board[3], river_card];
                let oop_scores: Vec<u32> = oop_combos
                    .iter()
                    .map(|c| evaluate_fast(&[c.0, c.1, river_board[0], river_board[1], river_board[2], river_board[3], river_board[4]]))
                    .collect();
                let ip_scores: Vec<u32> = ip_combos
                    .iter()
                    .map(|c| evaluate_fast(&[c.0, c.1, river_board[0], river_board[1], river_board[2], river_board[3], river_board[4]]))
                    .collect();

                let (valid_ip_h, valid_oop_h) = match br_player {
                    Player::OOP => {
                        let v: Vec<u16> = ip_combos
                            .iter()
                            .enumerate()
                            .filter(|(_, ip)| {
                                let oop = &oop_combos[hand_idx];
                                oop.0 != ip.0 && oop.0 != ip.1 && oop.1 != ip.0 && oop.1 != ip.1
                                    && ip.0 != river_card && ip.1 != river_card
                            })
                            .map(|(j, _)| j as u16)
                            .collect();
                        (v, Vec::new())
                    }
                    Player::IP => {
                        let v: Vec<u16> = oop_combos
                            .iter()
                            .enumerate()
                            .filter(|(_, oop)| {
                                let ip = &ip_combos[hand_idx];
                                ip.0 != oop.0 && ip.0 != oop.1 && ip.1 != oop.0 && ip.1 != oop.1
                                    && oop.0 != river_card && oop.1 != river_card
                            })
                            .map(|(i, _)| i as u16)
                            .collect();
                        (Vec::new(), v)
                    }
                };

                total_value += br_traverse_river(
                    &children[ci],
                    br_player,
                    hand_idx,
                    &new_opp_reach,
                    oop_combos,
                    ip_combos,
                    &oop_scores,
                    &ip_scores,
                    &valid_ip_h,
                    &valid_oop_h,
                    oop_cfr,
                    ip_cfr,
                    strat_buf,
                    is_br,
                );
            }

            if valid_count > 0 {
                total_value / valid_count as f64
            } else {
                0.0
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
                    // Best response: pick max
                    let mut best = f64::NEG_INFINITY;
                    for a in 0..num_actions {
                        let v = br_traverse_turn(
                            &children[a], br_player, hand_idx, opp_reach,
                            oop_combos, ip_combos, oop_blockers, ip_blockers,
                            board, oop_cfr, ip_cfr, strat_buf, is_br,
                        );
                        if v > best {
                            best = v;
                        }
                    }
                    best
                } else {
                    // Average strategy
                    let cfr = match br_player {
                        Player::OOP => oop_cfr,
                        Player::IP => ip_cfr,
                    };
                    cfr.average_strategy(nid, hand_idx, strat_buf);
                    let mut node_value = 0.0;
                    for a in 0..num_actions {
                        let v = br_traverse_turn(
                            &children[a], br_player, hand_idx, opp_reach,
                            oop_combos, ip_combos, oop_blockers, ip_blockers,
                            board, oop_cfr, ip_cfr, strat_buf, is_br,
                        );
                        node_value += strat_buf[a] as f64 * v;
                    }
                    node_value
                }
            } else {
                // Opponent uses average strategy
                let opp_cfr_ref = match br_player {
                    Player::OOP => ip_cfr,
                    Player::IP => oop_cfr,
                };
                let num_opp = opp_reach.len();
                let mut node_value = 0.0;

                for a in 0..num_actions {
                    let mut new_opp_reach = vec![0.0f64; num_opp];
                    for j in 0..num_opp {
                        if opp_reach[j] > 0.0 {
                            opp_cfr_ref.average_strategy(nid, j, strat_buf);
                            new_opp_reach[j] = opp_reach[j] * strat_buf[a] as f64;
                        }
                    }
                    node_value += br_traverse_turn(
                        &children[a], br_player, hand_idx, &new_opp_reach,
                        oop_combos, ip_combos, oop_blockers, ip_blockers,
                        board, oop_cfr, ip_cfr, strat_buf, is_br,
                    );
                }
                node_value
            }
        }
    }
}

/// Best-response / avg-strategy traversal for river subtrees.
#[allow(clippy::too_many_arguments)]
fn br_traverse_river(
    node: &TreeNode,
    br_player: Player,
    hand_idx: usize,
    opp_reach: &[f64],
    oop_combos: &[Combo],
    ip_combos: &[Combo],
    oop_scores: &[u32],
    ip_scores: &[u32],
    valid_ip_for_oop_h: &[u16],
    valid_oop_for_ip_h: &[u16],
    oop_cfr: &FlatCfr,
    ip_cfr: &FlatCfr,
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
                    let win_payoff = *pot - my_invested;
                    let lose_payoff = -my_invested;
                    let tie_payoff = *pot / 2.0 - my_invested;
                    let mut value = 0.0;

                    match br_player {
                        Player::OOP => {
                            let my_score = oop_scores[hand_idx];
                            for &j in valid_ip_for_oop_h {
                                let j = j as usize;
                                if opp_reach[j] < 1e-10 { continue; }
                                let opp_score = ip_scores[j];
                                let payoff = if my_score > opp_score { win_payoff }
                                    else if my_score < opp_score { lose_payoff }
                                    else { tie_payoff };
                                value += opp_reach[j] * payoff;
                            }
                        }
                        Player::IP => {
                            let my_score = ip_scores[hand_idx];
                            for &i in valid_oop_for_ip_h {
                                let i = i as usize;
                                if opp_reach[i] < 1e-10 { continue; }
                                let opp_score = oop_scores[i];
                                let payoff = if my_score > opp_score { win_payoff }
                                    else if my_score < opp_score { lose_payoff }
                                    else { tie_payoff };
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

            if *player == br_player {
                if is_br {
                    let mut best = f64::NEG_INFINITY;
                    for a in 0..num_actions {
                        let v = br_traverse_river(
                            &children[a], br_player, hand_idx, opp_reach,
                            oop_combos, ip_combos, oop_scores, ip_scores,
                            valid_ip_for_oop_h, valid_oop_for_ip_h,
                            oop_cfr, ip_cfr, strat_buf, is_br,
                        );
                        if v > best { best = v; }
                    }
                    best
                } else {
                    let cfr = match br_player {
                        Player::OOP => oop_cfr,
                        Player::IP => ip_cfr,
                    };
                    cfr.average_strategy(nid, hand_idx, strat_buf);
                    let mut node_value = 0.0;
                    for a in 0..num_actions {
                        let v = br_traverse_river(
                            &children[a], br_player, hand_idx, opp_reach,
                            oop_combos, ip_combos, oop_scores, ip_scores,
                            valid_ip_for_oop_h, valid_oop_for_ip_h,
                            oop_cfr, ip_cfr, strat_buf, is_br,
                        );
                        node_value += strat_buf[a] as f64 * v;
                    }
                    node_value
                }
            } else {
                let opp_cfr_ref = match br_player {
                    Player::OOP => ip_cfr,
                    Player::IP => oop_cfr,
                };
                let num_opp = opp_reach.len();
                let mut node_value = 0.0;

                for a in 0..num_actions {
                    let mut new_opp_reach = vec![0.0f64; num_opp];
                    for j in 0..num_opp {
                        if opp_reach[j] > 0.0 {
                            opp_cfr_ref.average_strategy(nid, j, strat_buf);
                            new_opp_reach[j] = opp_reach[j] * strat_buf[a] as f64;
                        }
                    }
                    node_value += br_traverse_river(
                        &children[a], br_player, hand_idx, &new_opp_reach,
                        oop_combos, ip_combos, oop_scores, ip_scores,
                        valid_ip_for_oop_h, valid_oop_for_ip_h,
                        oop_cfr, ip_cfr, strat_buf, is_br,
                    );
                }
                node_value
            }
        }
        TreeNode::Chance { .. } => unreachable!("No chance nodes in river subtrees"),
    }
}

// ---------------------------------------------------------------------------
// Solution extraction
// ---------------------------------------------------------------------------

fn extract_solution(
    config: &TurnSolverConfig,
    tree: &TreeNode,
    oop_cfr: &FlatCfr,
    ip_cfr: &FlatCfr,
    oop_combos: &[Combo],
    ip_combos: &[Combo],
    _metas: &[crate::postflop_tree::NodeMeta],
) -> TurnSolution {
    // Compute exploitability
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

    let exploitability = compute_exploitability(
        tree,
        oop_cfr,
        ip_cfr,
        oop_combos,
        ip_combos,
        &oop_blockers,
        &ip_blockers,
        &config.board,
    );

    // Extract turn-level strategies (first few action nodes before chance)
    let mut strategies = Vec::new();
    extract_turn_strategies(tree, oop_cfr, ip_cfr, oop_combos, ip_combos, &mut strategies);

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

    TurnSolution {
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
    }
}

fn extract_turn_strategies(
    node: &TreeNode,
    oop_cfr: &FlatCfr,
    ip_cfr: &FlatCfr,
    oop_combos: &[Combo],
    ip_combos: &[Combo],
    strategies: &mut Vec<TurnNodeStrategy>,
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
            let (cfr, num_combos) = match player {
                Player::OOP => (oop_cfr, oop_combos.len()),
                Player::IP => (ip_cfr, ip_combos.len()),
            };

            let mut avg_buf = vec![0.0f32; num_actions];
            let frequencies: Vec<Vec<f64>> = (0..num_combos)
                .map(|h| {
                    cfr.average_strategy(nid, h, &mut avg_buf);
                    avg_buf[..num_actions].iter().map(|&v| v as f64).collect()
                })
                .collect();

            strategies.push(TurnNodeStrategy {
                node_id: *node_id,
                player: match player {
                    Player::OOP => "OOP".to_string(),
                    Player::IP => "IP".to_string(),
                },
                actions: actions.iter().map(|a| a.label()).collect(),
                frequencies,
            });

            for child in children {
                extract_turn_strategies(child, oop_cfr, ip_cfr, oop_combos, ip_combos, strategies);
            }
        }
        TreeNode::Chance { .. } => {
            // Don't recurse into river subtrees for turn-level strategy extraction
        }
        TreeNode::Terminal { .. } => {}
    }
}

fn empty_solution(config: &TurnSolverConfig) -> TurnSolution {
    let board_str = config
        .board
        .iter()
        .map(|&b| format!("{}", index_to_card(b)))
        .collect::<String>();

    TurnSolution {
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
    }
}

// ---------------------------------------------------------------------------
// Display
// ---------------------------------------------------------------------------

impl TurnSolution {
    pub fn display(&self) {
        use colored::Colorize;

        println!();
        println!(
            "  {} Turn Solution  |  Board: {}  |  Pot: {:.0}  |  Stack: {:.0}  |  {} iterations",
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

impl TurnSolution {
    pub fn cache_path(&self) -> std::path::PathBuf {
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
        let dir = std::path::Path::new(&home).join(".gto-cli").join("solver");
        std::fs::create_dir_all(&dir).ok();
        dir.join(format!(
            "turn_{}_{:.0}_{:.0}.json",
            self.board, self.starting_pot, self.effective_stack,
        ))
    }

    pub fn save_cache(&self) {
        if let Ok(json) = serde_json::to_string(self) {
            let path = self.cache_path();
            std::fs::write(path, json).ok();
        }
    }

    pub fn load_cache(board: &str, pot: f64, stack: f64) -> Option<TurnSolution> {
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
        let path = std::path::Path::new(&home)
            .join(".gto-cli")
            .join("solver")
            .join(format!("turn_{}_{:.0}_{:.0}.json", board, pot, stack));
        let data = std::fs::read_to_string(path).ok()?;
        serde_json::from_str(&data).ok()
    }
}
