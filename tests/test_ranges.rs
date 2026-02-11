use gto_cli::cards::*;
use gto_cli::ranges::*;

#[test]
fn test_combo_count_pair() {
    assert_eq!(combo_count("AA"), 6);
}

#[test]
fn test_combo_count_suited() {
    assert_eq!(combo_count("AKs"), 4);
}

#[test]
fn test_combo_count_offsuit() {
    assert_eq!(combo_count("AKo"), 12);
}

#[test]
fn test_parse_range_simple() {
    let result = parse_range("AA,KK,QQ");
    assert!(result.contains(&"AA".to_string()));
    assert!(result.contains(&"KK".to_string()));
    assert!(result.contains(&"QQ".to_string()));
}

#[test]
fn test_parse_range_plus_pairs() {
    let result = parse_range("TT+");
    assert!(result.contains(&"TT".to_string()));
    assert!(result.contains(&"JJ".to_string()));
    assert!(result.contains(&"QQ".to_string()));
    assert!(result.contains(&"KK".to_string()));
    assert!(result.contains(&"AA".to_string()));
    assert!(!result.contains(&"99".to_string()));
}

#[test]
fn test_parse_range_plus_suited() {
    let result = parse_range("ATs+");
    assert!(result.contains(&"ATs".to_string()));
    assert!(result.contains(&"AJs".to_string()));
    assert!(result.contains(&"AQs".to_string()));
    assert!(result.contains(&"AKs".to_string()));
    assert!(!result.contains(&"A9s".to_string()));
}

#[test]
fn test_parse_range_dash_pairs() {
    let result = parse_range("77-TT");
    assert!(result.contains(&"77".to_string()));
    assert!(result.contains(&"88".to_string()));
    assert!(result.contains(&"99".to_string()));
    assert!(result.contains(&"TT".to_string()));
    assert!(!result.contains(&"66".to_string()));
    assert!(!result.contains(&"JJ".to_string()));
}

#[test]
fn test_parse_range_dash_suited() {
    let result = parse_range("KTs-KQs");
    assert!(result.contains(&"KTs".to_string()));
    assert!(result.contains(&"KJs".to_string()));
    assert!(result.contains(&"KQs".to_string()));
    assert!(!result.contains(&"K9s".to_string()));
}

#[test]
fn test_parse_range_mixed() {
    let result = parse_range("AA, KK, AKs, AQs+");
    assert!(result.contains(&"AA".to_string()));
    assert!(result.contains(&"AKs".to_string()));
}

#[test]
fn test_range_from_top_pct_1() {
    let result = range_from_top_pct(1.0).unwrap();
    assert!(result.contains(&"AA".to_string()));
    assert!(result.len() <= 3);
}

#[test]
fn test_range_from_top_pct_10() {
    let result = range_from_top_pct(10.0).unwrap();
    let combos = total_combos(&result);
    assert!(combos <= (1326.0 * 0.12) as u32);
}

#[test]
fn test_range_from_top_pct_50() {
    let result = range_from_top_pct(50.0).unwrap();
    assert!(result.len() > 20);
}

#[test]
fn test_range_from_top_pct_invalid() {
    assert!(range_from_top_pct(0.0).is_err());
    assert!(range_from_top_pct(101.0).is_err());
}

#[test]
fn test_total_combos() {
    assert_eq!(total_combos(&["AA".to_string()]), 6);
    assert_eq!(
        total_combos(&["AA".to_string(), "KK".to_string()]),
        12
    );
    assert_eq!(total_combos(&["AKs".to_string()]), 4);
    assert_eq!(total_combos(&["AKo".to_string()]), 12);
}

#[test]
fn test_range_pct_all_pairs() {
    let pairs: Vec<String> = vec![
        "AA", "KK", "QQ", "JJ", "TT", "99", "88", "77", "66", "55", "44", "33", "22",
    ]
    .into_iter()
    .map(String::from)
    .collect();
    let pct = range_pct(&pairs);
    assert!((pct - (78.0 / 1326.0 * 100.0)).abs() < 0.1);
}

#[test]
fn test_blockers_remove() {
    let hero = vec![
        Card::new(Rank::Ace, Suit::Spades),
        Card::new(Rank::King, Suit::Hearts),
    ];
    let range: Vec<String> = vec!["AA", "KK", "QQ"]
        .into_iter()
        .map(String::from)
        .collect();
    let result = blockers_remove(&range, &hero);
    assert!(result.contains(&"AA".to_string())); // still has some combos
    assert!(result.contains(&"QQ".to_string())); // not blocked at all
}

#[test]
fn test_blocked_combos_count() {
    let hero = vec![
        Card::new(Rank::Ace, Suit::Spades),
        Card::new(Rank::Ace, Suit::Hearts),
    ];
    let count = blocked_combos("AA", &hero).unwrap();
    assert_eq!(count, 5); // holding 2 aces blocks 5 of 6 combos
}
