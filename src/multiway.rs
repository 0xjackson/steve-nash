use crate::error::{GtoError, GtoResult};

pub struct MultiwayBetAdvice {
    pub frequency: f64,
    pub sizing: String,
    pub reasoning: String,
}

pub fn multiway_defense_freq(num_players: usize, bet_size: f64, pot: f64) -> GtoResult<f64> {
    if pot <= 0.0 || num_players < 2 {
        return Err(GtoError::InvalidValue(
            "Invalid pot or player count".to_string(),
        ));
    }
    let base_mdf = pot / (pot + bet_size);
    let per_player = 1.0 - (1.0 - base_mdf).powf(1.0 / (num_players as f64 - 1.0));
    Ok(per_player)
}

pub fn multiway_cbet(num_players: usize, board_wetness: &str, position: &str) -> MultiwayBetAdvice {
    let ip = position.to_uppercase() == "IP";

    if num_players >= 4 {
        return MultiwayBetAdvice {
            frequency: 0.15,
            sizing: "50-66% pot".to_string(),
            reasoning: "4+ players \u{2014} rarely c-bet, need strong hands or strong draws"
                .to_string(),
        };
    }

    if num_players == 3 {
        if board_wetness == "dry" {
            let freq = if ip { 0.35 } else { 0.20 };
            return MultiwayBetAdvice {
                frequency: freq,
                sizing: "33% pot".to_string(),
                reasoning: "3-way dry board \u{2014} small sizing, reduced frequency".to_string(),
            };
        }
        if board_wetness == "wet" {
            let freq = if ip { 0.25 } else { 0.15 };
            return MultiwayBetAdvice {
                frequency: freq,
                sizing: "66-75% pot".to_string(),
                reasoning: "3-way wet board \u{2014} only strong hands/draws, larger sizing"
                    .to_string(),
            };
        }
        let freq = if ip { 0.30 } else { 0.20 };
        return MultiwayBetAdvice {
            frequency: freq,
            sizing: "50% pot".to_string(),
            reasoning: "3-way medium texture \u{2014} selective betting".to_string(),
        };
    }

    // 2 players - heads-up
    if board_wetness == "dry" {
        let freq = if ip { 0.65 } else { 0.45 };
        MultiwayBetAdvice {
            frequency: freq,
            sizing: "33% pot".to_string(),
            reasoning: "Heads-up dry \u{2014} standard c-bet".to_string(),
        }
    } else if board_wetness == "wet" {
        let freq = if ip { 0.45 } else { 0.30 };
        MultiwayBetAdvice {
            frequency: freq,
            sizing: "66-75% pot".to_string(),
            reasoning: "Heads-up wet \u{2014} polarized sizing".to_string(),
        }
    } else {
        let freq = if ip { 0.55 } else { 0.40 };
        MultiwayBetAdvice {
            frequency: freq,
            sizing: "50% pot".to_string(),
            reasoning: "Heads-up medium texture \u{2014} balanced".to_string(),
        }
    }
}

pub fn multiway_sizing(num_players: usize, board_wetness: &str) -> &'static str {
    if num_players >= 4 {
        return "66-75% pot (punish draws, narrow ranges)";
    }
    if num_players == 3 {
        if board_wetness == "dry" {
            return "25-33% pot";
        }
        return "50-66% pot";
    }
    if board_wetness == "dry" {
        return "25-33% pot";
    }
    if board_wetness == "wet" {
        return "66-75% pot";
    }
    "50% pot"
}

pub fn multiway_range_adjustment(num_players: usize) -> &'static str {
    if num_players >= 4 {
        "Tighten significantly. Need top 10-15% of range to continue. Draws need near-nut quality."
    } else if num_players == 3 {
        "Tighten moderately. Top pair needs good kicker. Draws should have nut potential."
    } else {
        "Standard heads-up ranges apply."
    }
}
