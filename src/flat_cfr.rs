//! Flat-array CFR+ engine for postflop solving.
//!
//! Replaces the HashMap-based `CfrTrainer` with contiguous f32 arrays for
//! ~5x memory reduction and better cache performance. Designed for turn and
//! flop solvers where info set counts reach millions.
//!
//! Layout: data is organized by *node*, where each node has a fixed number
//! of actions and hands. For node `n`, hand `h`, action `a`:
//!
//!   index = offsets[n] + h * num_actions[n] + a
//!
//! This keeps all hands at the same tree position contiguous for good
//! cache locality during CFR iteration.

/// Flat-array CFR+ storage.
///
/// Each "node" represents all info sets at one position in the game tree
/// (one per hand combo of the acting player). Regrets and cumulative
/// strategy weights are stored in parallel contiguous arrays.
#[derive(Clone)]
pub struct FlatCfr {
    regrets: Vec<f32>,
    cum_strategy: Vec<f32>,
    /// Number of legal actions at each node.
    num_actions: Vec<u8>,
    /// Number of hand combos at each node.
    num_hands: Vec<u16>,
    /// Start offset in the data arrays for each node.
    offsets: Vec<u32>,
}

impl FlatCfr {
    /// Create a new FlatCfr from a list of (num_actions, num_hands) per node.
    ///
    /// Nodes are indexed 0..nodes.len()-1. The order must match the node_ids
    /// used during CFR traversal.
    pub fn new(nodes: &[(u8, u16)]) -> Self {
        let mut offsets = Vec::with_capacity(nodes.len());
        let mut num_actions = Vec::with_capacity(nodes.len());
        let mut num_hands = Vec::with_capacity(nodes.len());
        let mut offset: u32 = 0;

        for &(actions, hands) in nodes {
            offsets.push(offset);
            num_actions.push(actions);
            num_hands.push(hands);
            offset += actions as u32 * hands as u32;
        }

        let total = offset as usize;
        FlatCfr {
            regrets: vec![0.0f32; total],
            cum_strategy: vec![0.0f32; total],
            num_actions,
            num_hands,
            offsets,
        }
    }

    /// Number of nodes in this instance.
    #[inline]
    pub fn num_nodes(&self) -> usize {
        self.offsets.len()
    }

    /// Number of actions at the given node.
    #[inline]
    pub fn node_num_actions(&self, node: usize) -> u8 {
        self.num_actions[node]
    }

    /// Total number of f32 entries (regrets or cum_strategy).
    pub fn total_entries(&self) -> usize {
        self.regrets.len()
    }

    /// Memory usage in bytes (both arrays).
    pub fn memory_bytes(&self) -> usize {
        self.regrets.len() * 4 * 2
            + self.num_actions.len()
            + self.num_hands.len() * 2
            + self.offsets.len() * 4
    }

    // -----------------------------------------------------------------------
    // Index helpers
    // -----------------------------------------------------------------------

    /// Base index for (node, hand) in the flat arrays.
    #[inline]
    fn base(&self, node: usize, hand: usize) -> usize {
        self.offsets[node] as usize + hand * self.num_actions[node] as usize
    }

    // -----------------------------------------------------------------------
    // Strategy computation
    // -----------------------------------------------------------------------

    /// Write the current strategy (from regret matching) into `out`.
    ///
    /// `out` must have length >= num_actions[node]. Uses CFR+ regret matching:
    /// proportional to positive regrets, uniform if all non-positive.
    #[inline]
    pub fn current_strategy(&self, node: usize, hand: usize, out: &mut [f32]) {
        let na = self.num_actions[node] as usize;
        let base = self.base(node, hand);
        let regrets = &self.regrets[base..base + na];

        let mut positive_sum: f32 = 0.0;
        for &r in regrets {
            positive_sum += r.max(0.0);
        }

        if positive_sum > 0.0 {
            let inv = 1.0 / positive_sum;
            for (i, &r) in regrets.iter().enumerate() {
                out[i] = r.max(0.0) * inv;
            }
        } else {
            let uniform = 1.0 / na as f32;
            for o in out[..na].iter_mut() {
                *o = uniform;
            }
        }
    }

    /// Write the average strategy (Nash equilibrium approximation) into `out`.
    ///
    /// `out` must have length >= num_actions[node].
    #[inline]
    pub fn average_strategy(&self, node: usize, hand: usize, out: &mut [f32]) {
        let na = self.num_actions[node] as usize;
        let base = self.base(node, hand);
        let cum = &self.cum_strategy[base..base + na];

        let total: f32 = cum.iter().sum();
        if total > 0.0 {
            let inv = 1.0 / total;
            for (i, &s) in cum.iter().enumerate() {
                out[i] = s * inv;
            }
        } else {
            let uniform = 1.0 / na as f32;
            for o in out[..na].iter_mut() {
                *o = uniform;
            }
        }
    }

    // -----------------------------------------------------------------------
    // CFR+ update
    // -----------------------------------------------------------------------

    /// Update regrets and cumulative strategy for one info set.
    ///
    /// - `action_values`: counterfactual value of each action (len = num_actions)
    /// - `node_value`: weighted value of the node under current strategy
    /// - `reach_prob`: probability of reaching this info set (for strategy weighting)
    ///
    /// Regrets are floored at 0.0 (CFR+).
    #[inline]
    pub fn update(
        &mut self,
        node: usize,
        hand: usize,
        action_values: &[f32],
        node_value: f32,
        reach_prob: f32,
    ) {
        let na = self.num_actions[node] as usize;
        let base = self.base(node, hand);

        // Read current strategy for accumulation
        let mut positive_sum: f32 = 0.0;
        for i in 0..na {
            positive_sum += self.regrets[base + i].max(0.0);
        }

        for i in 0..na {
            // Update regret (CFR+: floor at 0)
            let regret = action_values[i] - node_value;
            self.regrets[base + i] = (self.regrets[base + i] + regret).max(0.0);

            // Accumulate strategy weighted by reach probability
            let sigma = if positive_sum > 0.0 {
                self.regrets[base + i].max(0.0) / positive_sum
            } else {
                1.0 / na as f32
            };
            self.cum_strategy[base + i] += reach_prob * sigma;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_creates_correct_sizes() {
        let cfr = FlatCfr::new(&[(3, 100), (2, 200)]);
        assert_eq!(cfr.num_nodes(), 2);
        assert_eq!(cfr.total_entries(), 3 * 100 + 2 * 200);
        assert_eq!(cfr.node_num_actions(0), 3);
        assert_eq!(cfr.node_num_actions(1), 2);
    }

    #[test]
    fn initial_strategy_is_uniform() {
        let cfr = FlatCfr::new(&[(3, 10)]);
        let mut out = [0.0f32; 3];
        cfr.current_strategy(0, 0, &mut out);
        for &v in &out {
            assert!((v - 1.0 / 3.0).abs() < 1e-6);
        }
    }

    #[test]
    fn average_strategy_initially_uniform() {
        let cfr = FlatCfr::new(&[(2, 5)]);
        let mut out = [0.0f32; 2];
        cfr.average_strategy(0, 0, &mut out);
        assert!((out[0] - 0.5).abs() < 1e-6);
        assert!((out[1] - 0.5).abs() < 1e-6);
    }

    #[test]
    fn update_shifts_strategy() {
        let mut cfr = FlatCfr::new(&[(2, 1)]);

        // Action 0 has value 10, action 1 has value -5, node value = 2.5
        // (as if strategy was [0.5, 0.5])
        cfr.update(0, 0, &[10.0, -5.0], 2.5, 1.0);

        let mut out = [0.0f32; 2];
        cfr.current_strategy(0, 0, &mut out);
        // Regret for action 0 = max(0 + 10 - 2.5, 0) = 7.5
        // Regret for action 1 = max(0 + (-5 - 2.5), 0) = 0
        // Strategy: [1.0, 0.0]
        assert!((out[0] - 1.0).abs() < 1e-6);
        assert!(out[1] < 1e-6);
    }

    #[test]
    fn cfr_plus_floors_regret_at_zero() {
        let mut cfr = FlatCfr::new(&[(2, 1)]);

        // First update: give action 1 positive regret
        cfr.update(0, 0, &[-10.0, 5.0], 0.0, 1.0);
        // regret[0] = max(0 + -10, 0) = 0
        // regret[1] = max(0 + 5, 0) = 5

        // Second update: punish action 1
        cfr.update(0, 0, &[3.0, -20.0], 0.0, 1.0);
        // regret[0] = max(0 + 3, 0) = 3
        // regret[1] = max(5 + -20, 0) = 0  (floored!)

        let mut out = [0.0f32; 2];
        cfr.current_strategy(0, 0, &mut out);
        assert!((out[0] - 1.0).abs() < 1e-6);
        assert!(out[1] < 1e-6);
    }

    #[test]
    fn multiple_hands_independent() {
        let mut cfr = FlatCfr::new(&[(2, 3)]);

        // Update hand 0 to prefer action 0
        cfr.update(0, 0, &[10.0, 0.0], 5.0, 1.0);
        // Update hand 1 to prefer action 1
        cfr.update(0, 1, &[0.0, 10.0], 5.0, 1.0);
        // Hand 2 untouched

        let mut out = [0.0f32; 2];

        cfr.current_strategy(0, 0, &mut out);
        assert!(out[0] > 0.9);

        cfr.current_strategy(0, 1, &mut out);
        assert!(out[1] > 0.9);

        cfr.current_strategy(0, 2, &mut out);
        assert!((out[0] - 0.5).abs() < 1e-6); // still uniform
    }

    #[test]
    fn multiple_nodes_independent() {
        let mut cfr = FlatCfr::new(&[(3, 2), (2, 2)]);

        // Update node 0, hand 0
        cfr.update(0, 0, &[10.0, 0.0, 0.0], 3.33, 1.0);
        // Node 1 should be unaffected
        let mut out = [0.0f32; 2];
        cfr.current_strategy(1, 0, &mut out);
        assert!((out[0] - 0.5).abs() < 1e-6);
    }

    #[test]
    fn average_strategy_accumulates() {
        let mut cfr = FlatCfr::new(&[(2, 1)]);

        // Multiple updates accumulate into average strategy
        for _ in 0..10 {
            cfr.update(0, 0, &[5.0, 0.0], 2.5, 1.0);
        }

        let mut out = [0.0f32; 2];
        cfr.average_strategy(0, 0, &mut out);
        // Should favor action 0 in the average
        assert!(out[0] > out[1]);
    }

    #[test]
    fn memory_bytes_reasonable() {
        // 1000 nodes × 4 actions × 500 hands = 2M entries
        let nodes: Vec<(u8, u16)> = (0..1000).map(|_| (4u8, 500u16)).collect();
        let cfr = FlatCfr::new(&nodes);
        let mb = cfr.memory_bytes() as f64 / 1_000_000.0;
        // 2M entries × 4 bytes × 2 arrays = 16 MB + small overhead
        assert!(mb < 20.0, "Expected <20 MB, got {:.1} MB", mb);
        assert!(mb > 10.0, "Expected >10 MB, got {:.1} MB", mb);
    }
}
