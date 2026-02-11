use std::collections::HashMap;

use once_cell::sync::Lazy;
use serde::Deserialize;

use crate::error::{GtoError, GtoResult};

static RANGES_6MAX_JSON: &str = include_str!("../data/ranges_6max.json");
static RANGES_9MAX_JSON: &str = include_str!("../data/ranges_9max.json");

#[derive(Deserialize, Debug)]
struct RfiEntry {
    #[serde(rename = "raise")]
    raise_range: Vec<String>,
    open_pct: u32,
}

#[derive(Deserialize, Debug)]
struct VsRfiEntry {
    call: Vec<String>,
    #[serde(rename = "3bet")]
    three_bet: Vec<String>,
    fold: String,
}

#[derive(Deserialize, Debug)]
struct Vs3BetEntry {
    call: Vec<String>,
    #[serde(rename = "4bet")]
    four_bet: Vec<String>,
    fold: String,
}

#[derive(Deserialize, Debug)]
struct SqueezeEntry {
    squeeze: Vec<String>,
    fold: String,
}

#[derive(Deserialize, Debug)]
struct BbDefenseEntry {
    call: Vec<String>,
    #[serde(rename = "3bet")]
    three_bet: Vec<String>,
}

#[derive(Deserialize, Debug)]
struct RangeData {
    #[serde(rename = "RFI")]
    rfi: HashMap<String, RfiEntry>,
    #[serde(rename = "vs_RFI")]
    vs_rfi: HashMap<String, VsRfiEntry>,
    vs_3bet: HashMap<String, Vs3BetEntry>,
    squeeze: HashMap<String, SqueezeEntry>,
    bb_defense: HashMap<String, BbDefenseEntry>,
}

#[derive(Deserialize, Debug)]
struct RangeFile6Max {
    #[serde(rename = "6max")]
    data: RangeData,
}

#[derive(Deserialize, Debug)]
struct RangeFile9Max {
    #[serde(rename = "9max")]
    data: RangeData,
}

static DATA_6MAX: Lazy<RangeData> = Lazy::new(|| {
    let file: RangeFile6Max = serde_json::from_str(RANGES_6MAX_JSON).expect("Failed to parse 6max ranges");
    file.data
});

static DATA_9MAX: Lazy<RangeData> = Lazy::new(|| {
    let file: RangeFile9Max = serde_json::from_str(RANGES_9MAX_JSON).expect("Failed to parse 9max ranges");
    file.data
});

fn get_data(table_size: &str) -> &'static RangeData {
    if table_size == "9max" {
        &DATA_9MAX
    } else {
        &DATA_6MAX
    }
}

pub const POSITIONS_6MAX: &[&str] = &["UTG", "HJ", "CO", "BTN", "SB", "BB"];
pub const POSITIONS_9MAX: &[&str] = &["UTG", "UTG1", "UTG2", "MP", "HJ", "CO", "BTN", "SB", "BB"];

pub fn positions_for(table_size: &str) -> &'static [&'static str] {
    if table_size == "6max" {
        POSITIONS_6MAX
    } else {
        POSITIONS_9MAX
    }
}

pub fn get_rfi_range(position: &str, table_size: &str) -> Vec<String> {
    let data = get_data(table_size);
    data.rfi
        .get(position)
        .map(|e| e.raise_range.clone())
        .unwrap_or_default()
}

pub fn get_rfi_pct(position: &str, table_size: &str) -> u32 {
    let data = get_data(table_size);
    data.rfi.get(position).map(|e| e.open_pct).unwrap_or(0)
}

#[derive(Debug, Clone)]
pub struct VsRfiResult {
    pub call: Vec<String>,
    pub three_bet: Vec<String>,
    pub fold: String,
}

pub fn get_vs_rfi_range(hero_pos: &str, villain_pos: &str, table_size: &str) -> VsRfiResult {
    let data = get_data(table_size);
    let key = format!("{}_vs_{}", hero_pos, villain_pos);

    if let Some(vs_rfi) = data.vs_rfi.get(&key) {
        return VsRfiResult {
            call: vs_rfi.call.clone(),
            three_bet: vs_rfi.three_bet.clone(),
            fold: vs_rfi.fold.clone(),
        };
    }

    // BB defense fallback
    if hero_pos == "BB" {
        let bb_key = format!("vs_{}", villain_pos);
        if let Some(bb_def) = data.bb_defense.get(&bb_key) {
            return VsRfiResult {
                call: bb_def.call.clone(),
                three_bet: bb_def.three_bet.clone(),
                fold: "default".to_string(),
            };
        }
    }

    VsRfiResult {
        call: vec![],
        three_bet: vec![],
        fold: "default".to_string(),
    }
}

#[derive(Debug, Clone)]
pub struct Vs3BetResult {
    pub call: Vec<String>,
    pub four_bet: Vec<String>,
    pub fold: String,
}

pub fn get_vs_3bet_range(hero_pos: &str, villain_pos: &str, table_size: &str) -> Vs3BetResult {
    let data = get_data(table_size);
    let key = format!("{}_vs_{}", hero_pos, villain_pos);

    if let Some(vs_3bet) = data.vs_3bet.get(&key) {
        return Vs3BetResult {
            call: vs_3bet.call.clone(),
            four_bet: vs_3bet.four_bet.clone(),
            fold: vs_3bet.fold.clone(),
        };
    }

    // Generic fallback
    let generic_key = format!("{}_vs_any", hero_pos);
    if let Some(vs_3bet) = data.vs_3bet.get(&generic_key) {
        return Vs3BetResult {
            call: vs_3bet.call.clone(),
            four_bet: vs_3bet.four_bet.clone(),
            fold: vs_3bet.fold.clone(),
        };
    }

    Vs3BetResult {
        call: vec![],
        four_bet: vec![],
        fold: "default".to_string(),
    }
}

pub fn get_squeeze_range(
    hero_pos: &str,
    raiser_pos: &str,
    caller_pos: &str,
    table_size: &str,
) -> Vec<String> {
    let data = get_data(table_size);
    let key = format!("{}_vs_{}_{}", hero_pos, raiser_pos, caller_pos);

    if let Some(sq) = data.squeeze.get(&key) {
        return sq.squeeze.clone();
    }

    // Fallback: find any key starting with hero_pos
    for (k, v) in &data.squeeze {
        if k.starts_with(&format!("{}_vs_", hero_pos)) {
            return v.squeeze.clone();
        }
    }

    vec![]
}

pub fn get_bb_defense(vs_position: &str, table_size: &str) -> VsRfiResult {
    let data = get_data(table_size);
    let key = format!("vs_{}", vs_position);
    if let Some(bb_def) = data.bb_defense.get(&key) {
        VsRfiResult {
            call: bb_def.call.clone(),
            three_bet: bb_def.three_bet.clone(),
            fold: "default".to_string(),
        }
    } else {
        VsRfiResult {
            call: vec![],
            three_bet: vec![],
            fold: "default".to_string(),
        }
    }
}

pub struct PreflopAction {
    pub action: String,
    pub hand: String,
    pub position: String,
    pub detail: String,
}

pub fn preflop_action(
    hand: &str,
    position: &str,
    situation: &str,
    villain_pos: Option<&str>,
    table_size: &str,
) -> GtoResult<PreflopAction> {
    match situation {
        "RFI" => {
            let rfi = get_rfi_range(position, table_size);
            if rfi.iter().any(|h| h == hand) {
                Ok(PreflopAction {
                    action: "RAISE".to_string(),
                    hand: hand.to_string(),
                    position: position.to_string(),
                    detail: format!("Open raise from {}", position),
                })
            } else {
                Ok(PreflopAction {
                    action: "FOLD".to_string(),
                    hand: hand.to_string(),
                    position: position.to_string(),
                    detail: format!("Not in {} opening range", position),
                })
            }
        }
        "vs_RFI" => {
            let vp = villain_pos.ok_or_else(|| {
                GtoError::InvalidValue("villain_pos required for vs_RFI".to_string())
            })?;
            let result = get_vs_rfi_range(position, vp, table_size);
            if result.three_bet.iter().any(|h| h == hand) {
                Ok(PreflopAction {
                    action: "3BET".to_string(),
                    hand: hand.to_string(),
                    position: position.to_string(),
                    detail: format!("3-bet vs {} open", vp),
                })
            } else if result.call.iter().any(|h| h == hand) {
                Ok(PreflopAction {
                    action: "CALL".to_string(),
                    hand: hand.to_string(),
                    position: position.to_string(),
                    detail: format!("Call {} open", vp),
                })
            } else {
                Ok(PreflopAction {
                    action: "FOLD".to_string(),
                    hand: hand.to_string(),
                    position: position.to_string(),
                    detail: format!("Fold vs {} open", vp),
                })
            }
        }
        "vs_3bet" => {
            let vp = villain_pos.ok_or_else(|| {
                GtoError::InvalidValue("villain_pos required for vs_3bet".to_string())
            })?;
            let result = get_vs_3bet_range(position, vp, table_size);
            if result.four_bet.iter().any(|h| h == hand) {
                Ok(PreflopAction {
                    action: "4BET".to_string(),
                    hand: hand.to_string(),
                    position: position.to_string(),
                    detail: format!("4-bet vs {} 3-bet", vp),
                })
            } else if result.call.iter().any(|h| h == hand) {
                Ok(PreflopAction {
                    action: "CALL".to_string(),
                    hand: hand.to_string(),
                    position: position.to_string(),
                    detail: format!("Call {} 3-bet", vp),
                })
            } else {
                Ok(PreflopAction {
                    action: "FOLD".to_string(),
                    hand: hand.to_string(),
                    position: position.to_string(),
                    detail: format!("Fold vs {} 3-bet", vp),
                })
            }
        }
        _ => Ok(PreflopAction {
            action: "FOLD".to_string(),
            hand: hand.to_string(),
            position: position.to_string(),
            detail: "Unknown situation".to_string(),
        }),
    }
}
