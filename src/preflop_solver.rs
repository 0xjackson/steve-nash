//! Preflop solver for full decision trees (open/3-bet/4-bet/5-bet).
//!
//! Solves 15 independent 2-player spots for 6-max using CFR+.
//! Each spot is an (opener, responder) position pair with a 5-node game tree:
//!
//! ```text
//! Node 100 (Opener): Open 2.5bb / Fold
//!   └─ Open → Node 101 (Responder): 3-Bet 7.5bb / Call 2.5bb / Fold
//!        ├─ 3-Bet → Node 102 (Opener): 4-Bet 18bb / Call 7.5bb / Fold
//!        │    └─ 4-Bet → Node 103 (Responder): All-In / Call 18bb / Fold
//!        │         └─ All-In → Node 104 (Opener): Call / Fold
//!        ├─ Call → Terminal (showdown with equity realization)
//!        └─ Fold → Terminal (opener wins blinds)
//! ```

use serde::{Deserialize, Serialize};

use crate::cfr::{CfrTrainer, InfoSetKey};
use crate::game_tree::{
    bucket_to_hand, precompute_equity_table, EquityTable, NUM_HANDS,
};
use crate::ranges::combo_count;

// ---------------------------------------------------------------------------
// Node IDs for the preflop game tree
// ---------------------------------------------------------------------------

const NODE_OPEN: u16 = 100;       // Opener: Open / Fold
const NODE_VS_OPEN: u16 = 101;    // Responder: 3-Bet / Call / Fold
const NODE_VS_3BET: u16 = 102;    // Opener: 4-Bet / Call / Fold
const NODE_VS_4BET: u16 = 103;    // Responder: All-In / Call / Fold
const NODE_VS_5BET: u16 = 104;    // Opener: Call / Fold

// Action counts per node
const ACTIONS_OPEN: usize = 2;     // Open, Fold
const ACTIONS_VS_OPEN: usize = 3;  // 3-Bet, Call, Fold
const ACTIONS_VS_3BET: usize = 3;  // 4-Bet, Call, Fold
const ACTIONS_VS_4BET: usize = 3;  // All-In, Call, Fold
const ACTIONS_VS_5BET: usize = 2;  // Call, Fold

// ---------------------------------------------------------------------------
// Position
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Position {
    UTG,
    HJ,
    CO,
    BTN,
    SB,
    BB,
}

impl Position {
    /// Returns the blind amount posted by this position (in bb).
    pub fn blind_amount(&self) -> f64 {
        match self {
            Position::SB => 0.5,
            Position::BB => 1.0,
            _ => 0.0,
        }
    }

    /// Whether this position is in-position relative to the other.
    /// Later positions act later postflop (more IP).
    pub fn is_ip_vs(&self, other: &Position) -> bool {
        self.seat_index() > other.seat_index()
    }

    fn seat_index(&self) -> usize {
        match self {
            Position::UTG => 0,
            Position::HJ => 1,
            Position::CO => 2,
            Position::BTN => 3,
            Position::SB => 4,
            Position::BB => 5,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Position::UTG => "UTG",
            Position::HJ => "HJ",
            Position::CO => "CO",
            Position::BTN => "BTN",
            Position::SB => "SB",
            Position::BB => "BB",
        }
    }

    pub fn from_str(s: &str) -> Option<Position> {
        match s.to_uppercase().as_str() {
            "UTG" => Some(Position::UTG),
            "HJ" => Some(Position::HJ),
            "CO" => Some(Position::CO),
            "BTN" => Some(Position::BTN),
            "SB" => Some(Position::SB),
            "BB" => Some(Position::BB),
            _ => None,
        }
    }
}

impl std::fmt::Display for Position {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

// ---------------------------------------------------------------------------
// All 6-max spots
// ---------------------------------------------------------------------------

/// Returns all 15 (opener, responder) position pairs for 6-max.
pub fn all_6max_spots() -> Vec<(Position, Position)> {
    use Position::*;
    vec![
        // UTG opens vs 5 positions
        (UTG, HJ), (UTG, CO), (UTG, BTN), (UTG, SB), (UTG, BB),
        // HJ opens vs 4 positions
        (HJ, CO), (HJ, BTN), (HJ, SB), (HJ, BB),
        // CO opens vs 3 positions
        (CO, BTN), (CO, SB), (CO, BB),
        // BTN opens vs 2 positions
        (BTN, SB), (BTN, BB),
        // SB opens vs BB
        (SB, BB),
    ]
}

// ---------------------------------------------------------------------------
// Payoff model
// ---------------------------------------------------------------------------

/// Payoff parameters for a preflop spot.
pub struct PreflopPayoffs {
    pub stack_bb: f64,
    pub rake: f64,           // fraction (0.0 - 1.0)
    pub dead_money: f64,     // blinds from players not in the spot
    pub opener_blind: f64,   // blind posted by opener
    pub responder_blind: f64,// blind posted by responder
    pub open_size: f64,      // 2.5bb
    pub three_bet_size: f64, // 7.5bb (3x open)
    pub four_bet_size: f64,  // 18.75bb (2.5x 3-bet)
    pub ip_is_opener: bool,  // whether opener is IP
    pub eq_realization: f64, // OOP equity realization factor (0.95)
}

impl PreflopPayoffs {
    pub fn new(opener: Position, responder: Position, stack_bb: f64, rake_pct: f64) -> Self {
        let opener_blind = opener.blind_amount();
        let responder_blind = responder.blind_amount();

        // Dead money = total blinds (1.5bb) minus what opener and responder post
        let dead_money = 1.5 - opener_blind - responder_blind;

        // IP determination: in general, opener is earlier position,
        // but SB vs BB is special (BB is IP postflop)
        let ip_is_opener = opener.is_ip_vs(&responder);

        PreflopPayoffs {
            stack_bb,
            rake: rake_pct / 100.0,
            dead_money,
            opener_blind,
            responder_blind,
            open_size: 2.5,
            three_bet_size: 7.5,
            four_bet_size: 18.75,
            ip_is_opener,
            eq_realization: 0.95,
        }
    }

    /// Opener folds preflop (loses their blind).
    #[inline]
    pub fn opener_folds_pre(&self) -> f64 {
        -self.opener_blind
    }

    /// Opener opens, responder folds. Opener wins responder's blind + dead money.
    #[inline]
    pub fn responder_folds_to_open(&self) -> f64 {
        self.responder_blind + self.dead_money
    }

    /// Opener opens, responder calls. Showdown with equity realization.
    /// Pot = open_size * 2 + dead_money. Each committed open_size.
    #[inline]
    pub fn flat_call_showdown(&self, opener_equity: f64) -> f64 {
        let pot = self.open_size * 2.0 + self.dead_money;
        let eq = self.apply_realization(opener_equity, true);
        eq * pot * (1.0 - self.rake) - self.open_size
    }

    /// Opener opens, responder 3-bets, opener folds. Opener loses open_size.
    #[inline]
    pub fn opener_folds_to_3bet(&self) -> f64 {
        -self.open_size
    }

    /// Opener opens, responder 3-bets, opener calls. Showdown.
    /// Pot = three_bet_size * 2 + dead_money.
    #[inline]
    pub fn call_3bet_showdown(&self, opener_equity: f64) -> f64 {
        let pot = self.three_bet_size * 2.0 + self.dead_money;
        let eq = self.apply_realization(opener_equity, true);
        eq * pot * (1.0 - self.rake) - self.three_bet_size
    }

    /// Opener opens, responder 3-bets, opener 4-bets, responder folds.
    /// Opener wins responder's 3-bet + dead money.
    #[inline]
    pub fn responder_folds_to_4bet(&self) -> f64 {
        self.three_bet_size + self.dead_money
    }

    /// Opener opens, responder 3-bets, opener 4-bets, responder calls.
    /// Pot = four_bet_size * 2 + dead_money.
    #[inline]
    pub fn call_4bet_showdown(&self, opener_equity: f64) -> f64 {
        let pot = self.four_bet_size * 2.0 + self.dead_money;
        let eq = self.apply_realization(opener_equity, true);
        eq * pot * (1.0 - self.rake) - self.four_bet_size
    }

    /// Responder shoves all-in, opener folds. Opener loses 4-bet amount.
    #[inline]
    pub fn opener_folds_to_5bet(&self) -> f64 {
        -self.four_bet_size
    }

    /// Responder shoves all-in, opener calls. All-in showdown.
    /// Pot = stack * 2 + dead_money.
    #[inline]
    pub fn allin_showdown(&self, opener_equity: f64) -> f64 {
        let pot = self.stack_bb * 2.0 + self.dead_money;
        let eq = self.apply_realization(opener_equity, true);
        eq * pot * (1.0 - self.rake) - self.stack_bb
    }

    /// Apply equity realization: IP gets raw equity, OOP gets equity * factor.
    #[inline]
    fn apply_realization(&self, opener_equity: f64, is_opener: bool) -> f64 {
        if is_opener {
            if self.ip_is_opener {
                opener_equity
            } else {
                opener_equity * self.eq_realization
            }
        } else {
            // responder perspective — not used directly
            let resp_eq = 1.0 - opener_equity;
            if self.ip_is_opener {
                resp_eq * self.eq_realization
            } else {
                resp_eq
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Spot result
// ---------------------------------------------------------------------------

/// Strategy arrays for all 5 nodes of a single preflop spot.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreflopSpotResult {
    pub opener: Position,
    pub responder: Position,
    /// Node 100: [open_freq; 169] (index 0 = open probability)
    pub open_strategy: Vec<f64>,
    /// Node 101: [3bet_freq, call_freq; 169] — fold = 1 - 3bet - call
    pub vs_open_3bet: Vec<f64>,
    pub vs_open_call: Vec<f64>,
    /// Node 102: [4bet_freq, call_freq; 169]
    pub vs_3bet_4bet: Vec<f64>,
    pub vs_3bet_call: Vec<f64>,
    /// Node 103: [allin_freq, call_freq; 169]
    pub vs_4bet_allin: Vec<f64>,
    pub vs_4bet_call: Vec<f64>,
    /// Node 104: [call_freq; 169]
    pub vs_5bet_call: Vec<f64>,
    pub exploitability: f64,
    pub iterations: usize,
}

impl PreflopSpotResult {
    /// Open range percentage (weighted by combos).
    pub fn open_pct(&self) -> f64 {
        weighted_pct(&self.open_strategy)
    }

    /// 3-bet percentage (weighted by combos).
    pub fn three_bet_pct(&self) -> f64 {
        weighted_pct(&self.vs_open_3bet)
    }

    /// Flat call vs open percentage (weighted by combos).
    pub fn flat_call_pct(&self) -> f64 {
        weighted_pct(&self.vs_open_call)
    }
}

fn weighted_pct(strategy: &[f64]) -> f64 {
    let mut total_combos = 0.0;
    let mut action_combos = 0.0;
    for i in 0..NUM_HANDS {
        let c = combo_count(&bucket_to_hand(i)) as f64;
        total_combos += c;
        action_combos += c * strategy[i];
    }
    if total_combos > 0.0 {
        action_combos / total_combos * 100.0
    } else {
        0.0
    }
}

// ---------------------------------------------------------------------------
// Core solver
// ---------------------------------------------------------------------------

/// Solve a single preflop spot (one opener vs one responder).
pub fn solve_preflop_spot(
    opener: Position,
    responder: Position,
    stack_bb: f64,
    iterations: usize,
    rake_pct: f64,
    table: &EquityTable,
) -> PreflopSpotResult {
    let payoffs = PreflopPayoffs::new(opener, responder, stack_bb, rake_pct);
    let mut trainer = CfrTrainer::new();

    // Pre-create all info sets.
    for h in 0..NUM_HANDS {
        let hb = h as u16;
        trainer.get_or_create(&InfoSetKey { hand_bucket: hb, node_id: NODE_OPEN }, ACTIONS_OPEN);
        trainer.get_or_create(&InfoSetKey { hand_bucket: hb, node_id: NODE_VS_OPEN }, ACTIONS_VS_OPEN);
        trainer.get_or_create(&InfoSetKey { hand_bucket: hb, node_id: NODE_VS_3BET }, ACTIONS_VS_3BET);
        trainer.get_or_create(&InfoSetKey { hand_bucket: hb, node_id: NODE_VS_4BET }, ACTIONS_VS_4BET);
        trainer.get_or_create(&InfoSetKey { hand_bucket: hb, node_id: NODE_VS_5BET }, ACTIONS_VS_5BET);
    }

    // Run CFR+ iterations.
    for _ in 0..iterations {
        preflop_cfr_iteration(&mut trainer, table, &payoffs);
    }

    // Extract average strategies.
    let mut open_strategy = vec![0.0; NUM_HANDS];
    let mut vs_open_3bet = vec![0.0; NUM_HANDS];
    let mut vs_open_call = vec![0.0; NUM_HANDS];
    let mut vs_3bet_4bet = vec![0.0; NUM_HANDS];
    let mut vs_3bet_call = vec![0.0; NUM_HANDS];
    let mut vs_4bet_allin = vec![0.0; NUM_HANDS];
    let mut vs_4bet_call = vec![0.0; NUM_HANDS];
    let mut vs_5bet_call = vec![0.0; NUM_HANDS];

    for h in 0..NUM_HANDS {
        let hb = h as u16;

        let s = trainer.get_average_strategy(&InfoSetKey { hand_bucket: hb, node_id: NODE_OPEN }, ACTIONS_OPEN);
        open_strategy[h] = s[0];

        let s = trainer.get_average_strategy(&InfoSetKey { hand_bucket: hb, node_id: NODE_VS_OPEN }, ACTIONS_VS_OPEN);
        vs_open_3bet[h] = s[0];
        vs_open_call[h] = s[1];

        let s = trainer.get_average_strategy(&InfoSetKey { hand_bucket: hb, node_id: NODE_VS_3BET }, ACTIONS_VS_3BET);
        vs_3bet_4bet[h] = s[0];
        vs_3bet_call[h] = s[1];

        let s = trainer.get_average_strategy(&InfoSetKey { hand_bucket: hb, node_id: NODE_VS_4BET }, ACTIONS_VS_4BET);
        vs_4bet_allin[h] = s[0];
        vs_4bet_call[h] = s[1];

        let s = trainer.get_average_strategy(&InfoSetKey { hand_bucket: hb, node_id: NODE_VS_5BET }, ACTIONS_VS_5BET);
        vs_5bet_call[h] = s[0];
    }

    let exploitability = compute_preflop_exploitability(
        &open_strategy, &vs_open_3bet, &vs_open_call,
        &vs_3bet_4bet, &vs_3bet_call,
        &vs_4bet_allin, &vs_4bet_call,
        &vs_5bet_call,
        table, &payoffs,
    );

    PreflopSpotResult {
        opener,
        responder,
        open_strategy,
        vs_open_3bet,
        vs_open_call,
        vs_3bet_4bet,
        vs_3bet_call,
        vs_4bet_allin,
        vs_4bet_call,
        vs_5bet_call,
        exploitability,
        iterations,
    }
}

/// One CFR+ iteration: alternating updates for opener and responder.
fn preflop_cfr_iteration(
    trainer: &mut CfrTrainer,
    table: &EquityTable,
    payoffs: &PreflopPayoffs,
) {
    // --- Snapshot responder strategies (nodes 101, 103) ---
    let resp_101: Vec<[f64; 3]> = (0..NUM_HANDS)
        .map(|h| {
            let s = trainer.get_strategy(&InfoSetKey { hand_bucket: h as u16, node_id: NODE_VS_OPEN }, ACTIONS_VS_OPEN);
            [s[0], s[1], s[2]]
        })
        .collect();

    let resp_103: Vec<[f64; 3]> = (0..NUM_HANDS)
        .map(|h| {
            let s = trainer.get_strategy(&InfoSetKey { hand_bucket: h as u16, node_id: NODE_VS_4BET }, ACTIONS_VS_4BET);
            [s[0], s[1], s[2]]
        })
        .collect();

    // --- Update opener nodes (100, 102, 104) ---
    // Snapshot opener strategies for self-reference
    let opener_102: Vec<[f64; 3]> = (0..NUM_HANDS)
        .map(|h| {
            let s = trainer.get_strategy(&InfoSetKey { hand_bucket: h as u16, node_id: NODE_VS_3BET }, ACTIONS_VS_3BET);
            [s[0], s[1], s[2]]
        })
        .collect();

    let opener_104: Vec<[f64; 2]> = (0..NUM_HANDS)
        .map(|h| {
            let s = trainer.get_strategy(&InfoSetKey { hand_bucket: h as u16, node_id: NODE_VS_5BET }, ACTIONS_VS_5BET);
            [s[0], s[1]]
        })
        .collect();

    for op in 0..NUM_HANDS {
        let op_key_100 = InfoSetKey { hand_bucket: op as u16, node_id: NODE_OPEN };
        let op_strat_100 = trainer.get_strategy(&op_key_100, ACTIONS_OPEN);

        // Compute EV of opening vs folding at node 100
        let fold_ev_100 = payoffs.opener_folds_pre();
        let mut open_ev = 0.0;
        let mut total_w = 0.0;

        for resp in 0..NUM_HANDS {
            let w = table.weight(op, resp);
            if w < 1e-10 { continue; }
            total_w += w;

            let eq = table.eq(op, resp);

            // Responder actions at node 101: 3-bet, call, fold
            let r_3bet = resp_101[resp][0];
            let r_call = resp_101[resp][1];
            let r_fold = resp_101[resp][2];

            // EV when responder folds
            let ev_resp_fold = payoffs.responder_folds_to_open();

            // EV when responder calls (flat)
            let ev_resp_call = payoffs.flat_call_showdown(eq);

            // EV when responder 3-bets → go to node 102
            let ev_resp_3bet = compute_ev_after_3bet(
                eq, &opener_102[op], &resp_103[resp], &opener_104[op], payoffs,
            );

            let ev_open_vs_resp = r_fold * ev_resp_fold + r_call * ev_resp_call + r_3bet * ev_resp_3bet;
            open_ev += w * ev_open_vs_resp;
        }

        if total_w > 0.0 {
            open_ev /= total_w;
        }

        let node_value_100 = op_strat_100[0] * open_ev + op_strat_100[1] * fold_ev_100;
        let data = trainer.get_or_create(&op_key_100, ACTIONS_OPEN);
        data.update(&[open_ev, fold_ev_100], node_value_100, 1.0);

        // --- Update node 102 (opener vs 3-bet) ---
        // EV is conditional on reaching node 102 (responder 3-bet)
        let op_key_102 = InfoSetKey { hand_bucket: op as u16, node_id: NODE_VS_3BET };
        let op_strat_102 = opener_102[op];

        let mut fourbet_ev = 0.0;
        let mut call3bet_ev = 0.0;
        let fold3bet_ev = payoffs.opener_folds_to_3bet();
        let mut total_w_102 = 0.0;

        for resp in 0..NUM_HANDS {
            let w = table.weight(op, resp);
            if w < 1e-10 { continue; }
            let r_3bet = resp_101[resp][0];
            if r_3bet < 1e-10 { continue; }
            let wt = w * r_3bet;
            total_w_102 += wt;

            let eq = table.eq(op, resp);

            // Call 3-bet → showdown
            call3bet_ev += wt * payoffs.call_3bet_showdown(eq);

            // 4-bet → node 103
            let ev_4bet = compute_ev_after_4bet(eq, &resp_103[resp], &opener_104[op], payoffs);
            fourbet_ev += wt * ev_4bet;
        }

        if total_w_102 > 0.0 {
            fourbet_ev /= total_w_102;
            call3bet_ev /= total_w_102;
        }

        let node_value_102 = op_strat_102[0] * fourbet_ev + op_strat_102[1] * call3bet_ev + op_strat_102[2] * fold3bet_ev;
        let data = trainer.get_or_create(&op_key_102, ACTIONS_VS_3BET);
        data.update(&[fourbet_ev, call3bet_ev, fold3bet_ev], node_value_102, 1.0);

        // --- Update node 104 (opener vs 5-bet/all-in) ---
        let op_key_104 = InfoSetKey { hand_bucket: op as u16, node_id: NODE_VS_5BET };
        let op_strat_104 = opener_104[op];

        let mut call5bet_ev = 0.0;
        let fold5bet_ev = payoffs.opener_folds_to_5bet();
        let mut total_w_104 = 0.0;

        for resp in 0..NUM_HANDS {
            let w = table.weight(op, resp);
            if w < 1e-10 { continue; }
            let r_3bet = resp_101[resp][0];
            if r_3bet < 1e-10 { continue; }
            let r_allin = resp_103[resp][0];
            if r_allin < 1e-10 { continue; }
            // Also need opener's 4-bet prob to reach this node
            let op_4bet = opener_102[op][0];
            if op_4bet < 1e-10 { continue; }

            let wt = w * r_3bet * op_4bet * r_allin;
            total_w_104 += wt;

            let eq = table.eq(op, resp);
            call5bet_ev += wt * payoffs.allin_showdown(eq);
        }

        if total_w_104 > 0.0 {
            call5bet_ev /= total_w_104;
        }

        let node_value_104 = op_strat_104[0] * call5bet_ev + op_strat_104[1] * fold5bet_ev;
        let data = trainer.get_or_create(&op_key_104, ACTIONS_VS_5BET);
        data.update(&[call5bet_ev, fold5bet_ev], node_value_104, 1.0);
    }

    // --- Now snapshot opener strategies for responder update ---
    let opener_100: Vec<[f64; 2]> = (0..NUM_HANDS)
        .map(|h| {
            let s = trainer.get_strategy(&InfoSetKey { hand_bucket: h as u16, node_id: NODE_OPEN }, ACTIONS_OPEN);
            [s[0], s[1]]
        })
        .collect();

    let opener_102_new: Vec<[f64; 3]> = (0..NUM_HANDS)
        .map(|h| {
            let s = trainer.get_strategy(&InfoSetKey { hand_bucket: h as u16, node_id: NODE_VS_3BET }, ACTIONS_VS_3BET);
            [s[0], s[1], s[2]]
        })
        .collect();

    let opener_104_new: Vec<[f64; 2]> = (0..NUM_HANDS)
        .map(|h| {
            let s = trainer.get_strategy(&InfoSetKey { hand_bucket: h as u16, node_id: NODE_VS_5BET }, ACTIONS_VS_5BET);
            [s[0], s[1]]
        })
        .collect();

    // --- Update responder nodes (101, 103) ---
    for resp in 0..NUM_HANDS {
        // --- Node 101: Responder vs open ---
        let resp_key_101 = InfoSetKey { hand_bucket: resp as u16, node_id: NODE_VS_OPEN };
        let resp_strat_101 = resp_101[resp];

        let mut threebet_ev = 0.0;
        let mut call_ev = 0.0;
        let resp_fold_ev = -payoffs.responder_blind;

        let mut total_w_101 = 0.0;

        for op in 0..NUM_HANDS {
            let w = table.weight(op, resp);
            if w < 1e-10 { continue; }
            let op_open = opener_100[op][0];
            if op_open < 1e-10 { continue; }
            let wt = w * op_open;
            total_w_101 += wt;

            let eq = table.eq(op, resp);
            let resp_eq = 1.0 - eq;

            // Call → showdown from responder's perspective
            let pot_flat = payoffs.open_size * 2.0 + payoffs.dead_money;
            let resp_eq_real = if payoffs.ip_is_opener {
                resp_eq
            } else {
                resp_eq * payoffs.eq_realization
            };
            let ev_call = resp_eq_real * pot_flat * (1.0 - payoffs.rake) - payoffs.open_size;
            call_ev += wt * ev_call;

            // 3-bet → subtree from responder's perspective
            let ev_3bet = compute_resp_ev_after_3bet(
                eq, &opener_102_new[op], &resp_103[resp], &opener_104_new[op], payoffs,
            );
            threebet_ev += wt * ev_3bet;
        }

        if total_w_101 > 0.0 {
            threebet_ev /= total_w_101;
            call_ev /= total_w_101;
        }

        let node_value_101 = resp_strat_101[0] * threebet_ev + resp_strat_101[1] * call_ev + resp_strat_101[2] * resp_fold_ev;
        let data = trainer.get_or_create(&resp_key_101, ACTIONS_VS_OPEN);
        data.update(&[threebet_ev, call_ev, resp_fold_ev], node_value_101, 1.0);

        // --- Node 103: Responder vs 4-bet ---
        let resp_key_103 = InfoSetKey { hand_bucket: resp as u16, node_id: NODE_VS_4BET };
        let resp_strat_103 = resp_103[resp];

        let mut allin_ev = 0.0;
        let mut call4bet_ev = 0.0;
        let resp_fold_4bet_ev = -payoffs.three_bet_size;
        let mut total_w_103 = 0.0;

        for op in 0..NUM_HANDS {
            let w = table.weight(op, resp);
            if w < 1e-10 { continue; }
            let op_open = opener_100[op][0];
            if op_open < 1e-10 { continue; }
            let op_4bet = opener_102_new[op][0];
            if op_4bet < 1e-10 { continue; }
            let r_3bet = resp_101[resp][0];
            if r_3bet < 1e-10 { continue; }

            let wt = w * op_open * r_3bet * op_4bet;
            total_w_103 += wt;

            let eq = table.eq(op, resp);
            let resp_eq = 1.0 - eq;

            // Call 4-bet → showdown from responder's perspective
            let pot_4bet = payoffs.four_bet_size * 2.0 + payoffs.dead_money;
            let resp_eq_real = if payoffs.ip_is_opener {
                resp_eq
            } else {
                resp_eq * payoffs.eq_realization
            };
            call4bet_ev += wt * (resp_eq_real * pot_4bet * (1.0 - payoffs.rake) - payoffs.four_bet_size);

            // All-in → opener decides at node 104
            let op_call_5bet = opener_104_new[op][0];
            let op_fold_5bet = opener_104_new[op][1];

            // Responder wins opener's 4-bet if opener folds
            let ev_op_folds = payoffs.four_bet_size + payoffs.dead_money;
            // All-in showdown from responder's perspective
            let pot_allin = payoffs.stack_bb * 2.0 + payoffs.dead_money;
            let ev_allin_showdown = if payoffs.ip_is_opener {
                resp_eq
            } else {
                resp_eq * payoffs.eq_realization
            } * pot_allin * (1.0 - payoffs.rake) - payoffs.stack_bb;

            allin_ev += wt * (op_fold_5bet * ev_op_folds + op_call_5bet * ev_allin_showdown);
        }

        if total_w_103 > 0.0 {
            allin_ev /= total_w_103;
            call4bet_ev /= total_w_103;
        }

        let node_value_103 = resp_strat_103[0] * allin_ev + resp_strat_103[1] * call4bet_ev + resp_strat_103[2] * resp_fold_4bet_ev;
        let data = trainer.get_or_create(&resp_key_103, ACTIONS_VS_4BET);
        data.update(&[allin_ev, call4bet_ev, resp_fold_4bet_ev], node_value_103, 1.0);
    }
}

/// Compute opener's EV after responder 3-bets (for opener's node 100 update).
/// This walks through nodes 102 → 103 → 104.
#[inline]
fn compute_ev_after_3bet(
    equity: f64,
    opener_102: &[f64; 3],
    resp_103: &[f64; 3],
    opener_104: &[f64; 2],
    payoffs: &PreflopPayoffs,
) -> f64 {
    let fold_ev = payoffs.opener_folds_to_3bet();
    let call_ev = payoffs.call_3bet_showdown(equity);
    let fourbet_ev = compute_ev_after_4bet(equity, resp_103, opener_104, payoffs);

    opener_102[0] * fourbet_ev + opener_102[1] * call_ev + opener_102[2] * fold_ev
}

/// Compute opener's EV after opener 4-bets (nodes 103 → 104).
#[inline]
fn compute_ev_after_4bet(
    equity: f64,
    resp_103: &[f64; 3],
    opener_104: &[f64; 2],
    payoffs: &PreflopPayoffs,
) -> f64 {
    let resp_fold_ev = payoffs.responder_folds_to_4bet();
    let resp_call_ev = payoffs.call_4bet_showdown(equity);

    // Responder all-in → opener at node 104
    let call_allin_ev = payoffs.allin_showdown(equity);
    let fold_allin_ev = payoffs.opener_folds_to_5bet();
    let ev_vs_allin = opener_104[0] * call_allin_ev + opener_104[1] * fold_allin_ev;

    resp_103[0] * ev_vs_allin + resp_103[1] * resp_call_ev + resp_103[2] * resp_fold_ev
}

/// Compute responder's EV after 3-betting (for responder's node 101 update).
#[inline]
fn compute_resp_ev_after_3bet(
    opener_equity: f64,
    opener_102: &[f64; 3],
    resp_103: &[f64; 3],
    opener_104: &[f64; 2],
    payoffs: &PreflopPayoffs,
) -> f64 {
    let resp_eq = 1.0 - opener_equity;

    // Opener folds to 3-bet: responder wins opener's open + dead money
    let ev_op_folds = payoffs.open_size + payoffs.dead_money;

    // Opener calls 3-bet: showdown from responder's perspective
    let pot_3bet = payoffs.three_bet_size * 2.0 + payoffs.dead_money;
    let resp_eq_real = if payoffs.ip_is_opener {
        resp_eq
    } else {
        resp_eq * payoffs.eq_realization
    };
    let ev_op_calls = resp_eq_real * pot_3bet * (1.0 - payoffs.rake) - payoffs.three_bet_size;

    // Opener 4-bets: responder at node 103
    let ev_op_4bets = compute_resp_ev_after_4bet(opener_equity, resp_103, opener_104, payoffs);

    opener_102[0] * ev_op_4bets + opener_102[1] * ev_op_calls + opener_102[2] * ev_op_folds
}

/// Compute responder's EV after opener 4-bets (for node 103 subtree).
#[inline]
fn compute_resp_ev_after_4bet(
    opener_equity: f64,
    resp_103: &[f64; 3],
    opener_104: &[f64; 2],
    payoffs: &PreflopPayoffs,
) -> f64 {
    let resp_eq = 1.0 - opener_equity;

    // Responder folds to 4-bet
    let fold_ev = -payoffs.three_bet_size;

    // Responder calls 4-bet → showdown
    let pot_4bet = payoffs.four_bet_size * 2.0 + payoffs.dead_money;
    let resp_eq_real = if payoffs.ip_is_opener {
        resp_eq
    } else {
        resp_eq * payoffs.eq_realization
    };
    let call_ev = resp_eq_real * pot_4bet * (1.0 - payoffs.rake) - payoffs.four_bet_size;

    // Responder all-in → opener at node 104
    let op_call = opener_104[0];
    let op_fold = opener_104[1];
    let ev_op_folds = payoffs.four_bet_size + payoffs.dead_money;
    let pot_allin = payoffs.stack_bb * 2.0 + payoffs.dead_money;
    let ev_allin_showdown = if payoffs.ip_is_opener {
        resp_eq
    } else {
        resp_eq * payoffs.eq_realization
    } * pot_allin * (1.0 - payoffs.rake) - payoffs.stack_bb;
    let allin_ev = op_fold * ev_op_folds + op_call * ev_allin_showdown;

    resp_103[0] * allin_ev + resp_103[1] * call_ev + resp_103[2] * fold_ev
}

// ---------------------------------------------------------------------------
// Exploitability
// ---------------------------------------------------------------------------

/// Compute exploitability for the preflop spot.
/// Sum of best-response gains for both players across all 5 nodes.
pub fn compute_preflop_exploitability(
    open_strat: &[f64],
    vs_open_3bet: &[f64],
    vs_open_call: &[f64],
    vs_3bet_4bet: &[f64],
    vs_3bet_call: &[f64],
    vs_4bet_allin: &[f64],
    vs_4bet_call: &[f64],
    vs_5bet_call: &[f64],
    table: &EquityTable,
    payoffs: &PreflopPayoffs,
) -> f64 {
    // Opener best response (nodes 100, 102, 104)
    let mut opener_gain = 0.0;
    let mut opener_combos = 0.0;

    for op in 0..NUM_HANDS {
        let combos = combo_count(&bucket_to_hand(op)) as f64;
        opener_combos += combos;

        // --- Node 100 best response ---
        let fold_ev = payoffs.opener_folds_pre();
        let mut open_ev = 0.0;
        let mut total_w = 0.0;

        for resp in 0..NUM_HANDS {
            let w = table.weight(op, resp);
            if w < 1e-10 { continue; }
            total_w += w;

            let eq = table.eq(op, resp);
            let r_3bet = vs_open_3bet[resp];
            let r_call = vs_open_call[resp];
            let r_fold = 1.0 - r_3bet - r_call;

            let op_102 = [vs_3bet_4bet[op], vs_3bet_call[op], 1.0 - vs_3bet_4bet[op] - vs_3bet_call[op]];
            let r_103 = [vs_4bet_allin[resp], vs_4bet_call[resp], 1.0 - vs_4bet_allin[resp] - vs_4bet_call[resp]];
            let op_104 = [vs_5bet_call[op], 1.0 - vs_5bet_call[op]];

            let ev_resp_fold = payoffs.responder_folds_to_open();
            let ev_resp_call = payoffs.flat_call_showdown(eq);
            let ev_resp_3bet = compute_ev_after_3bet(eq, &op_102, &r_103, &op_104, payoffs);

            open_ev += w * (r_fold * ev_resp_fold + r_call * ev_resp_call + r_3bet * ev_resp_3bet);
        }
        if total_w > 0.0 { open_ev /= total_w; }

        let current_ev = open_strat[op] * open_ev + (1.0 - open_strat[op]) * fold_ev;
        let best_ev = open_ev.max(fold_ev);
        opener_gain += combos * (best_ev - current_ev);
    }

    // Responder best response (nodes 101, 103)
    let mut resp_gain = 0.0;
    let mut resp_combos = 0.0;

    for resp in 0..NUM_HANDS {
        let combos = combo_count(&bucket_to_hand(resp)) as f64;
        resp_combos += combos;

        // --- Node 101 best response ---
        let resp_fold_ev = -payoffs.responder_blind;
        let mut threebet_ev = 0.0;
        let mut call_ev = 0.0;
        let mut total_w = 0.0;

        for op in 0..NUM_HANDS {
            let w = table.weight(op, resp);
            if w < 1e-10 { continue; }
            let op_open = open_strat[op];
            if op_open < 1e-10 { continue; }
            let wt = w * op_open;
            total_w += wt;

            let eq = table.eq(op, resp);
            let resp_eq = 1.0 - eq;

            let pot_flat = payoffs.open_size * 2.0 + payoffs.dead_money;
            let resp_eq_real = if payoffs.ip_is_opener { resp_eq } else { resp_eq * payoffs.eq_realization };
            call_ev += wt * (resp_eq_real * pot_flat * (1.0 - payoffs.rake) - payoffs.open_size);

            let op_102 = [vs_3bet_4bet[op], vs_3bet_call[op], 1.0 - vs_3bet_4bet[op] - vs_3bet_call[op]];
            let r_103 = [vs_4bet_allin[resp], vs_4bet_call[resp], 1.0 - vs_4bet_allin[resp] - vs_4bet_call[resp]];
            let op_104 = [vs_5bet_call[op], 1.0 - vs_5bet_call[op]];

            threebet_ev += wt * compute_resp_ev_after_3bet(eq, &op_102, &r_103, &op_104, payoffs);
        }
        if total_w > 0.0 {
            threebet_ev /= total_w;
            call_ev /= total_w;
        }

        let current_ev = vs_open_3bet[resp] * threebet_ev + vs_open_call[resp] * call_ev
            + (1.0 - vs_open_3bet[resp] - vs_open_call[resp]) * resp_fold_ev;
        let best_ev = threebet_ev.max(call_ev).max(resp_fold_ev);
        resp_gain += combos * (best_ev - current_ev);
    }

    let opener_exploit = if opener_combos > 0.0 { opener_gain / opener_combos } else { 0.0 };
    let resp_exploit = if resp_combos > 0.0 { resp_gain / resp_combos } else { 0.0 };

    (opener_exploit + resp_exploit) / 2.0
}

// ---------------------------------------------------------------------------
// Batch solving + disk cache
// ---------------------------------------------------------------------------

/// Complete solution for all spots at a given table size.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreflopSolution {
    pub table_size: String,
    pub stack_bb: f64,
    pub rake_pct: f64,
    pub iterations: usize,
    pub spots: Vec<PreflopSpotResult>,
}

/// Solve all 15 6-max preflop spots.
pub fn solve_preflop_6max(
    stack_bb: f64,
    iterations: usize,
    rake_pct: f64,
) -> PreflopSolution {
    use colored::Colorize;

    println!("  Computing equity table...");
    let table = precompute_equity_table(2000);
    println!("  Equity table ready.\n");

    let spots_config = all_6max_spots();
    let mut spots = Vec::with_capacity(spots_config.len());

    for (i, (opener, responder)) in spots_config.iter().enumerate() {
        print!(
            "  [{}/{}] Solving {} vs {} ...",
            i + 1,
            spots_config.len(),
            opener.as_str().bold(),
            responder.as_str().bold(),
        );
        let result = solve_preflop_spot(*opener, *responder, stack_bb, iterations, rake_pct, &table);
        println!(
            " done (exploit: {:.4} bb, open: {:.1}%, 3bet: {:.1}%)",
            result.exploitability,
            result.open_pct(),
            result.three_bet_pct(),
        );
        spots.push(result);
    }

    PreflopSolution {
        table_size: "6max".to_string(),
        stack_bb,
        rake_pct,
        iterations,
        spots,
    }
}

impl PreflopSolution {
    /// Find the spot result for a given (opener, responder) pair.
    pub fn find_spot(&self, opener: Position, responder: Position) -> Option<&PreflopSpotResult> {
        self.spots.iter().find(|s| s.opener == opener && s.responder == responder)
    }

    /// Get the cache file path for this solution.
    pub fn cache_path(&self) -> std::path::PathBuf {
        let dir = dirs_cache_dir();
        dir.join(format!(
            "preflop_{}_{}bb_{}pct.json",
            self.table_size,
            self.stack_bb as u64,
            self.rake_pct as u64,
        ))
    }

    /// Save solution to disk cache.
    pub fn save(&self) -> std::io::Result<()> {
        let path = self.cache_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string(self)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
        std::fs::write(&path, json)
    }

    /// Load solution from disk cache.
    pub fn load(table_size: &str, stack_bb: f64, rake_pct: f64) -> std::io::Result<Self> {
        let dir = dirs_cache_dir();
        let path = dir.join(format!(
            "preflop_{}_{}bb_{}pct.json",
            table_size,
            stack_bb as u64,
            rake_pct as u64,
        ));
        let json = std::fs::read_to_string(&path)?;
        serde_json::from_str(&json)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))
    }
}

fn dirs_cache_dir() -> std::path::PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    std::path::PathBuf::from(home).join(".gto-cli").join("solver")
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn position_blinds() {
        assert_eq!(Position::SB.blind_amount(), 0.5);
        assert_eq!(Position::BB.blind_amount(), 1.0);
        assert_eq!(Position::UTG.blind_amount(), 0.0);
        assert_eq!(Position::BTN.blind_amount(), 0.0);
    }

    #[test]
    fn position_ip() {
        assert!(Position::BTN.is_ip_vs(&Position::CO));
        assert!(Position::BB.is_ip_vs(&Position::SB));
        assert!(!Position::UTG.is_ip_vs(&Position::BTN));
    }

    #[test]
    fn dead_money_calculation() {
        // UTG vs BB: opener=UTG (0), responder=BB (1), dead=1.5-0-1=0.5
        let p = PreflopPayoffs::new(Position::UTG, Position::BB, 100.0, 0.0);
        assert!((p.dead_money - 0.5).abs() < 1e-9);
        assert!((p.opener_blind - 0.0).abs() < 1e-9);
        assert!((p.responder_blind - 1.0).abs() < 1e-9);

        // SB vs BB: opener=SB (0.5), responder=BB (1), dead=1.5-0.5-1=0
        let p = PreflopPayoffs::new(Position::SB, Position::BB, 100.0, 0.0);
        assert!((p.dead_money - 0.0).abs() < 1e-9);
        assert!((p.opener_blind - 0.5).abs() < 1e-9);
        assert!((p.responder_blind - 1.0).abs() < 1e-9);

        // BTN vs SB: opener=BTN (0), responder=SB (0.5), dead=1.5-0-0.5=1.0
        let p = PreflopPayoffs::new(Position::BTN, Position::SB, 100.0, 0.0);
        assert!((p.dead_money - 1.0).abs() < 1e-9);
    }

    #[test]
    fn payoff_opener_fold() {
        let p = PreflopPayoffs::new(Position::UTG, Position::BB, 100.0, 0.0);
        assert!((p.opener_folds_pre() - 0.0).abs() < 1e-9);

        let p = PreflopPayoffs::new(Position::SB, Position::BB, 100.0, 0.0);
        assert!((p.opener_folds_pre() - (-0.5)).abs() < 1e-9);
    }

    #[test]
    fn payoff_responder_folds_to_open() {
        let p = PreflopPayoffs::new(Position::UTG, Position::BB, 100.0, 0.0);
        // Opener wins BB's blind (1.0) + dead money (0.5) = 1.5
        assert!((p.responder_folds_to_open() - 1.5).abs() < 1e-9);

        let p = PreflopPayoffs::new(Position::SB, Position::BB, 100.0, 0.0);
        // Opener wins BB's blind (1.0) + dead money (0) = 1.0
        assert!((p.responder_folds_to_open() - 1.0).abs() < 1e-9);
    }

    #[test]
    fn all_spots_count() {
        assert_eq!(all_6max_spots().len(), 15);
    }

    #[test]
    fn position_roundtrip() {
        for pos in &[Position::UTG, Position::HJ, Position::CO, Position::BTN, Position::SB, Position::BB] {
            assert_eq!(Position::from_str(pos.as_str()), Some(*pos));
        }
    }
}
