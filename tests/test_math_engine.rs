use gto_cli::math_engine::*;

#[test]
fn test_pot_odds_basic() {
    let result = pot_odds(100.0, 50.0).unwrap();
    assert!((result - 0.25).abs() < 0.001);
}

#[test]
fn test_pot_odds_half_pot() {
    let result = pot_odds(100.0, 50.0).unwrap();
    assert!((result - 0.25).abs() < 0.001);
}

#[test]
fn test_pot_odds_full_pot() {
    let result = pot_odds(100.0, 100.0).unwrap();
    assert!((result - 1.0 / 3.0).abs() < 0.001);
}

#[test]
fn test_pot_odds_invalid() {
    assert!(pot_odds(0.0, 50.0).is_err());
}

#[test]
fn test_implied_odds_basic() {
    let result = implied_odds(100.0, 50.0, 100.0).unwrap();
    let po = pot_odds(100.0, 50.0).unwrap();
    assert!(result < po);
}

#[test]
fn test_implied_odds_zero_future() {
    let imp = implied_odds(100.0, 50.0, 0.0).unwrap();
    let po = pot_odds(100.0, 50.0).unwrap();
    assert!((imp - po).abs() < 0.001);
}

#[test]
fn test_reverse_implied_odds_basic() {
    let result = reverse_implied_odds(100.0, 50.0, 100.0).unwrap();
    let po = pot_odds(100.0, 50.0).unwrap();
    assert!(result > po);
}

#[test]
fn test_ev_positive() {
    let result = ev(0.5, 100.0, 50.0);
    assert!(result > 0.0);
}

#[test]
fn test_ev_break_even() {
    let equity = pot_odds(100.0, 50.0).unwrap();
    let result = ev(equity, 100.0, 50.0);
    assert!(result.abs() < 0.01);
}

#[test]
fn test_ev_negative() {
    let result = ev(0.1, 100.0, 100.0);
    assert!(result < 0.0);
}

#[test]
fn test_mdf_half_pot() {
    let result = mdf(50.0, 100.0).unwrap();
    assert!((result - 2.0 / 3.0).abs() < 0.001);
}

#[test]
fn test_mdf_full_pot() {
    let result = mdf(100.0, 100.0).unwrap();
    assert!((result - 0.5).abs() < 0.001);
}

#[test]
fn test_mdf_overbet() {
    let result = mdf(200.0, 100.0).unwrap();
    assert!((result - 1.0 / 3.0).abs() < 0.001);
}

#[test]
fn test_fold_equity_profitable() {
    let result = fold_equity(0.6, 100.0, 75.0);
    assert!(result > 0.0);
}

#[test]
fn test_fold_equity_unprofitable() {
    let result = fold_equity(0.2, 100.0, 75.0);
    assert!(result < 0.0);
}

#[test]
fn test_spr_low() {
    let result = spr(200.0, 100.0).unwrap();
    assert_eq!(result.zone, SprZone::Low);
    assert!((result.ratio - 2.0).abs() < 0.01);
}

#[test]
fn test_spr_medium() {
    let result = spr(700.0, 100.0).unwrap();
    assert_eq!(result.zone, SprZone::Medium);
}

#[test]
fn test_spr_high() {
    let result = spr(1500.0, 100.0).unwrap();
    assert_eq!(result.zone, SprZone::High);
}

#[test]
fn test_spr_str() {
    let result = spr(200.0, 100.0).unwrap();
    let s = format!("{}", result);
    assert!(s.contains("2.0"));
}

#[test]
fn test_bluff_to_value_half_pot() {
    let result = bluff_to_value_ratio(50.0, 100.0).unwrap();
    assert!((result - 1.0 / 3.0).abs() < 0.001);
}

#[test]
fn test_bluff_to_value_full_pot() {
    let result = bluff_to_value_ratio(100.0, 100.0).unwrap();
    assert!((result - 0.5).abs() < 0.001);
}

#[test]
fn test_break_even_pct() {
    let result = break_even_pct(100.0, 50.0).unwrap();
    assert!((result - 0.25).abs() < 0.001);
}

#[test]
fn test_effective_stack_two() {
    assert!((effective_stack(&[100.0, 200.0]).unwrap() - 100.0).abs() < 0.01);
}

#[test]
fn test_effective_stack_three() {
    assert!((effective_stack(&[50.0, 100.0, 200.0]).unwrap() - 100.0).abs() < 0.01);
}

#[test]
fn test_effective_stack_invalid() {
    assert!(effective_stack(&[100.0]).is_err());
}
