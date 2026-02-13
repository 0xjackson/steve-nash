//! Generates all 1,755 strategically distinct flop boards using suit isomorphism
//! reduction. Two flops are equivalent if they have the same ranks and the same
//! suit *pattern* (e.g., rainbow, two-tone, monotone). Canonical form maps suits
//! to indices 0,1,2,3 in order of first appearance.

use std::collections::BTreeSet;

/// Ranks indexed 0..13 mapping to 2,3,4,5,6,7,8,9,T,J,Q,K,A.
const RANK_CHARS: [char; 13] = ['2', '3', '4', '5', '6', '7', '8', '9', 'T', 'J', 'Q', 'K', 'A'];

/// Suits in canonical output order.
const SUIT_CHARS: [char; 4] = ['s', 'h', 'd', 'c'];

/// Generate all 1,755 strategically distinct flop boards.
///
/// Returns a `Vec<String>` of 6-character board strings (e.g., `"As7d2c"`),
/// sorted for deterministic output. Each string represents a canonical flop —
/// the unique representative for its suit isomorphism class.
pub fn generate_canonical_flops() -> Vec<String> {
    let mut canonical_set: BTreeSet<String> = BTreeSet::new();

    // Enumerate all 52C3 = 22,100 three-card combinations.
    // Card index 0..51: card i has rank i/4, suit i%4.
    for c1 in 0u8..52 {
        for c2 in (c1 + 1)..52 {
            for c3 in (c2 + 1)..52 {
                let cards = [(c1 / 4, c1 % 4), (c2 / 4, c2 % 4), (c3 / 4, c3 % 4)];
                let canonical = canonicalize(&cards);
                canonical_set.insert(canonical);
            }
        }
    }

    canonical_set.into_iter().collect()
}

/// Map a 3-card flop to its canonical string representation.
///
/// Two flops are equivalent under suit isomorphism if there exists a suit
/// relabeling that transforms one into the other. The canonical form is the
/// lexicographically smallest string obtainable by:
///
/// 1. Sorting cards by rank descending.
/// 2. Trying all orderings of cards within same-rank groups (handles paired/
///    trip boards where swapping same-rank cards is a valid suit relabeling).
/// 3. For each ordering, mapping suits to canonical indices 0,1,2,3 in order
///    of first appearance.
/// 4. Taking the lexicographic minimum across all orderings.
fn canonicalize(cards: &[(u8, u8); 3]) -> String {
    // Sort cards by rank descending
    let mut sorted = *cards;
    sorted.sort_by(|a, b| b.0.cmp(&a.0));

    // Generate all valid orderings by permuting within same-rank groups.
    let orderings = permutations_within_rank_groups(&sorted);

    orderings
        .into_iter()
        .map(|ordering| first_appearance_canonical(&ordering))
        .min()
        .unwrap()
}

/// Given cards sorted by rank descending, return all orderings that permute
/// cards within groups of the same rank.
fn permutations_within_rank_groups(cards: &[(u8, u8); 3]) -> Vec<[(u8, u8); 3]> {
    let (r0, r1, r2) = (cards[0].0, cards[1].0, cards[2].0);

    if r0 == r1 && r1 == r2 {
        // Trips: 3! = 6 permutations
        vec![
            [cards[0], cards[1], cards[2]],
            [cards[0], cards[2], cards[1]],
            [cards[1], cards[0], cards[2]],
            [cards[1], cards[2], cards[0]],
            [cards[2], cards[0], cards[1]],
            [cards[2], cards[1], cards[0]],
        ]
    } else if r0 == r1 {
        // First two share a rank: 2 permutations
        vec![
            [cards[0], cards[1], cards[2]],
            [cards[1], cards[0], cards[2]],
        ]
    } else if r1 == r2 {
        // Last two share a rank: 2 permutations
        vec![
            [cards[0], cards[1], cards[2]],
            [cards[0], cards[2], cards[1]],
        ]
    } else {
        // All distinct: 1 ordering
        vec![*cards]
    }
}

/// Compute canonical string by mapping suits to 0,1,2,3 in order of first
/// appearance (left to right).
fn first_appearance_canonical(cards: &[(u8, u8); 3]) -> String {
    let mut suit_map: [Option<u8>; 4] = [None; 4];
    let mut next_suit: u8 = 0;

    let mut result = String::with_capacity(6);
    for &(rank, suit) in cards {
        let canonical_suit = match suit_map[suit as usize] {
            Some(s) => s,
            None => {
                let s = next_suit;
                suit_map[suit as usize] = Some(s);
                next_suit += 1;
                s
            }
        };
        result.push(RANK_CHARS[rank as usize]);
        result.push(SUIT_CHARS[canonical_suit as usize]);
    }

    result
}

/// Return the strategic priority score for a canonical flop string.
/// Higher score = higher priority (should be solved first).
///
/// Priority tiers:
/// 1. A-high boards (most common in real play)
/// 2. K-high boards
/// 3. Broadway boards (all cards T+)
/// 4. Paired boards
/// 5. Medium connected boards
/// 6. Low/monotone/everything else
pub fn strategic_priority(board: &str) -> u32 {
    if board.len() != 6 {
        return 0;
    }

    let chars: Vec<char> = board.chars().collect();
    let r1 = rank_value(chars[0]);
    let r2 = rank_value(chars[2]);
    let r3 = rank_value(chars[4]);
    let high = r1.max(r2).max(r3);
    let suits = [chars[1], chars[3], chars[5]];
    let is_monotone = suits[0] == suits[1] && suits[1] == suits[2];
    let is_paired = r1 == r2 || r2 == r3 || r1 == r3;
    let is_broadway = r1 >= 10 && r2 >= 10 && r3 >= 10;

    // Base score from high card (A=14 -> 1400, K=13 -> 1300, etc.)
    let mut score = high as u32 * 100;

    // Bonus for broadway boards
    if is_broadway {
        score += 50;
    }

    // Bonus for paired boards
    if is_paired {
        score += 30;
    }

    // Penalty for monotone (less common, different strategy)
    if is_monotone {
        score -= 20;
    }

    // Bonus for connectedness (cards within 4 ranks of each other)
    let mut ranks = [r1, r2, r3];
    ranks.sort();
    let spread = ranks[2] - ranks[0];
    if spread <= 4 {
        score += 20;
    }

    score
}

fn rank_value(c: char) -> u8 {
    match c {
        '2' => 2, '3' => 3, '4' => 4, '5' => 5, '6' => 6, '7' => 7, '8' => 8,
        '9' => 9, 'T' => 10, 'J' => 11, 'Q' => 12, 'K' => 13, 'A' => 14,
        _ => 0,
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_canonical_flop_count() {
        let flops = generate_canonical_flops();
        assert_eq!(
            flops.len(),
            1755,
            "Expected 1,755 canonical flops, got {}",
            flops.len()
        );
    }

    #[test]
    fn test_canonical_flops_are_valid() {
        let flops = generate_canonical_flops();
        for board in &flops {
            assert_eq!(board.len(), 6, "Board '{}' should be 6 chars", board);
            let chars: Vec<char> = board.chars().collect();
            // Check rank chars are valid
            for i in (0..6).step_by(2) {
                assert!(
                    RANK_CHARS.contains(&chars[i]),
                    "Invalid rank char '{}' in board '{}'",
                    chars[i],
                    board
                );
            }
            // Check suit chars are valid
            for i in (1..6).step_by(2) {
                assert!(
                    SUIT_CHARS.contains(&chars[i]),
                    "Invalid suit char '{}' in board '{}'",
                    chars[i],
                    board
                );
            }
        }
    }

    #[test]
    fn test_canonical_flops_sorted_descending_rank() {
        let flops = generate_canonical_flops();
        for board in &flops {
            let chars: Vec<char> = board.chars().collect();
            let r1 = rank_value(chars[0]);
            let r2 = rank_value(chars[2]);
            let r3 = rank_value(chars[4]);
            assert!(
                r1 >= r2 && r2 >= r3,
                "Board '{}' ranks not in descending order: {} {} {}",
                board, r1, r2, r3
            );
        }
    }

    #[test]
    fn test_suit_isomorphism() {
        // These flops should all map to the same canonical form
        let a = canonicalize(&[(11, 0), (7, 1), (2, 2)]); // Ks9h4d
        let b = canonicalize(&[(11, 1), (7, 0), (2, 3)]); // Kh9s4c
        let c = canonicalize(&[(11, 2), (7, 3), (2, 0)]); // Kd9c4s
        assert_eq!(a, b, "Ks9h4d and Kh9s4c should canonicalize the same");
        assert_eq!(b, c, "Kh9s4c and Kd9c4s should canonicalize the same");
    }

    #[test]
    fn test_monotone_not_same_as_rainbow() {
        // Monotone: all same suit
        let mono = canonicalize(&[(11, 0), (7, 0), (2, 0)]); // Ks9s4s
        // Rainbow: all different suits
        let rainbow = canonicalize(&[(11, 0), (7, 1), (2, 2)]); // Ks9h4d
        assert_ne!(mono, rainbow, "Monotone and rainbow should be different canonical flops");
    }

    #[test]
    fn test_two_tone_variants() {
        // Two-tone with first two suited
        let a = canonicalize(&[(11, 0), (7, 0), (2, 1)]); // Ks9s4h
        // Two-tone with first and third suited
        let b = canonicalize(&[(11, 0), (7, 1), (2, 0)]); // Ks9h4s
        // Two-tone with last two suited
        let c = canonicalize(&[(11, 0), (7, 1), (2, 1)]); // Ks9h4h
        // These are different suit patterns (different strategic situations)
        // a: AAB, b: ABA, c: ABB — but after canonical mapping they should differ
        // based on which positions share suits
        assert_ne!(a, c, "Different two-tone patterns should be distinct");
    }

    #[test]
    fn test_strategic_priority_a_high_beats_low() {
        let a_high = strategic_priority("As7d2c");
        let low = strategic_priority("6s4d2c");
        assert!(a_high > low, "A-high should have higher priority than 6-high");
    }

    #[test]
    fn test_strategic_priority_broadway_bonus() {
        let broadway = strategic_priority("KsQhTd");
        let non_broadway = strategic_priority("Ks7d2c");
        assert!(broadway > non_broadway, "Broadway board should have higher priority");
    }
}
