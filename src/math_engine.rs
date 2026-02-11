use std::fmt;

use crate::error::{GtoError, GtoResult};

pub fn pot_odds(pot: f64, bet: f64) -> GtoResult<f64> {
    if pot <= 0.0 || bet <= 0.0 {
        return Err(GtoError::InvalidValue(
            "Pot and bet must be positive".to_string(),
        ));
    }
    Ok(bet / (pot + bet + bet))
}

pub fn implied_odds(pot: f64, bet: f64, expected_future: f64) -> GtoResult<f64> {
    if bet <= 0.0 {
        return Err(GtoError::InvalidValue("Bet must be positive".to_string()));
    }
    Ok(bet / (pot + bet + bet + expected_future))
}

pub fn reverse_implied_odds(pot: f64, bet: f64, risk: f64) -> GtoResult<f64> {
    if bet <= 0.0 {
        return Err(GtoError::InvalidValue("Bet must be positive".to_string()));
    }
    Ok((bet + risk) / (pot + bet + bet + risk))
}

pub fn ev(equity: f64, pot: f64, bet: f64) -> f64 {
    let win_amount = pot + bet;
    equity * win_amount - (1.0 - equity) * bet
}

pub fn mdf(bet_size: f64, pot_size: f64) -> GtoResult<f64> {
    if pot_size <= 0.0 {
        return Err(GtoError::InvalidValue("Pot must be positive".to_string()));
    }
    Ok(pot_size / (pot_size + bet_size))
}

pub fn fold_equity(fold_pct: f64, pot: f64, bet: f64) -> f64 {
    fold_pct * pot - (1.0 - fold_pct) * bet
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SprZone {
    Low,
    Medium,
    High,
}

impl fmt::Display for SprZone {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SprZone::Low => write!(f, "low"),
            SprZone::Medium => write!(f, "medium"),
            SprZone::High => write!(f, "high"),
        }
    }
}

pub struct SprResult {
    pub ratio: f64,
    pub zone: SprZone,
    pub guidance: &'static str,
}

impl fmt::Display for SprResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "SPR {:.1} ({})", self.ratio, self.zone)
    }
}

pub fn spr(stack: f64, pot: f64) -> GtoResult<SprResult> {
    if pot <= 0.0 {
        return Err(GtoError::InvalidValue("Pot must be positive".to_string()));
    }
    let ratio = stack / pot;
    let (zone, guidance) = if ratio <= 4.0 {
        (
            SprZone::Low,
            "Commit with top pair+. All-in pressure is standard.",
        )
    } else if ratio <= 10.0 {
        (
            SprZone::Medium,
            "Two pair+ for stacking. One pair hands play cautiously.",
        )
    } else {
        (
            SprZone::High,
            "Need very strong hands to stack off. Implied odds matter most.",
        )
    };
    Ok(SprResult {
        ratio,
        zone,
        guidance,
    })
}

pub fn bluff_to_value_ratio(bet_size: f64, pot_size: f64) -> GtoResult<f64> {
    if pot_size <= 0.0 {
        return Err(GtoError::InvalidValue("Pot must be positive".to_string()));
    }
    Ok(bet_size / (pot_size + bet_size))
}

pub fn break_even_pct(pot: f64, bet: f64) -> GtoResult<f64> {
    if pot + bet <= 0.0 {
        return Err(GtoError::InvalidValue(
            "Total pot must be positive".to_string(),
        ));
    }
    Ok(bet / (pot + bet + bet))
}

pub fn effective_stack(stacks: &[f64]) -> GtoResult<f64> {
    if stacks.len() < 2 {
        return Err(GtoError::InvalidValue(
            "Need at least 2 stacks".to_string(),
        ));
    }
    let mut sorted = stacks.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
    Ok(sorted[sorted.len() - 2])
}
