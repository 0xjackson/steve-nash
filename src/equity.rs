use std::fmt;

use rand::seq::SliceRandom;
use rayon::prelude::*;

use crate::cards::{hand_combos, Card, ALL_RANKS, ALL_SUITS};
use crate::error::{GtoError, GtoResult};
use crate::hand_evaluator::evaluate_hand;

pub struct EquityResult {
    pub win: f64,
    pub tie: f64,
    pub lose: f64,
    pub simulations: usize,
}

impl EquityResult {
    pub fn equity(&self) -> f64 {
        self.win + self.tie / 2.0
    }
}

impl fmt::Display for EquityResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Win {:.1}% | Tie {:.1}% | Lose {:.1}% (equity: {:.1}%)",
            self.win * 100.0,
            self.tie * 100.0,
            self.lose * 100.0,
            self.equity() * 100.0,
        )
    }
}

fn build_remaining_deck(dead: &[Card]) -> Vec<Card> {
    let dead_set: std::collections::HashSet<Card> = dead.iter().copied().collect();
    ALL_RANKS
        .iter()
        .flat_map(|&r| ALL_SUITS.iter().map(move |&s| Card::new(r, s)))
        .filter(|c| !dead_set.contains(c))
        .collect()
}

pub fn equity_vs_hand(
    hand1: &[Card],
    hand2: &[Card],
    board: Option<&[Card]>,
    simulations: usize,
) -> GtoResult<EquityResult> {
    let board = board.unwrap_or(&[]);
    let mut dead: Vec<Card> = Vec::new();
    dead.extend_from_slice(hand1);
    dead.extend_from_slice(hand2);
    dead.extend_from_slice(board);
    let remaining = build_remaining_deck(&dead);
    let cards_needed = 5 - board.len();

    let board_vec: Vec<Card> = board.to_vec();
    let h1: Vec<Card> = hand1.to_vec();
    let h2: Vec<Card> = hand2.to_vec();

    let results: Vec<(u64, u64, u64)> = (0..simulations)
        .into_par_iter()
        .map(|_| {
            let mut rng = rand::thread_rng();
            let mut deck = remaining.clone();
            deck.shuffle(&mut rng);
            let runout = &deck[..cards_needed];
            let mut full_board = board_vec.clone();
            full_board.extend_from_slice(runout);

            let r1 = evaluate_hand(&h1, &full_board).unwrap();
            let r2 = evaluate_hand(&h2, &full_board).unwrap();

            match r1.cmp(&r2) {
                std::cmp::Ordering::Greater => (1, 0, 0),
                std::cmp::Ordering::Equal => (0, 1, 0),
                std::cmp::Ordering::Less => (0, 0, 1),
            }
        })
        .collect();

    let (wins, ties, losses) = results
        .iter()
        .fold((0u64, 0u64, 0u64), |acc, &(w, t, l)| {
            (acc.0 + w, acc.1 + t, acc.2 + l)
        });

    let total = (wins + ties + losses) as f64;
    Ok(EquityResult {
        win: wins as f64 / total,
        tie: ties as f64 / total,
        lose: losses as f64 / total,
        simulations: total as usize,
    })
}

pub fn equity_vs_range(
    hand: &[Card],
    villain_range: &[String],
    board: Option<&[Card]>,
    simulations: usize,
) -> GtoResult<EquityResult> {
    let board = board.unwrap_or(&[]);
    let dead: std::collections::HashSet<Card> = hand.iter().chain(board.iter()).copied().collect();

    let mut all_combos: Vec<Vec<Card>> = Vec::new();
    for notation in villain_range {
        for (c1, c2) in hand_combos(notation)? {
            if !dead.contains(&c1) && !dead.contains(&c2) {
                all_combos.push(vec![c1, c2]);
            }
        }
    }

    if all_combos.is_empty() {
        return Err(GtoError::NoValidCombos);
    }

    let sims_per = (simulations / all_combos.len()).max(1);
    let board_vec: Vec<Card> = board.to_vec();
    let hero: Vec<Card> = hand.to_vec();

    let results: Vec<(u64, u64, u64)> = all_combos
        .par_iter()
        .map(|villain_hand| {
            let mut combo_dead: Vec<Card> = Vec::new();
            combo_dead.extend_from_slice(&hero);
            combo_dead.extend_from_slice(&board_vec);
            combo_dead.extend_from_slice(villain_hand);
            let remaining = build_remaining_deck(&combo_dead);
            let cards_needed = 5 - board_vec.len();

            let mut wins = 0u64;
            let mut ties = 0u64;
            let mut losses = 0u64;

            let mut rng = rand::thread_rng();
            for _ in 0..sims_per {
                let mut deck = remaining.clone();
                deck.shuffle(&mut rng);
                let runout = &deck[..cards_needed];
                let mut full_board = board_vec.clone();
                full_board.extend_from_slice(runout);

                let r1 = evaluate_hand(&hero, &full_board).unwrap();
                let r2 = evaluate_hand(villain_hand, &full_board).unwrap();

                match r1.cmp(&r2) {
                    std::cmp::Ordering::Greater => wins += 1,
                    std::cmp::Ordering::Equal => ties += 1,
                    std::cmp::Ordering::Less => losses += 1,
                }
            }

            (wins, ties, losses)
        })
        .collect();

    let (wins, ties, losses) = results
        .iter()
        .fold((0u64, 0u64, 0u64), |acc, &(w, t, l)| {
            (acc.0 + w, acc.1 + t, acc.2 + l)
        });

    let total = (wins + ties + losses) as f64;
    Ok(EquityResult {
        win: wins as f64 / total,
        tie: ties as f64 / total,
        lose: losses as f64 / total,
        simulations: total as usize,
    })
}
