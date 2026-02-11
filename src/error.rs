use thiserror::Error;

#[derive(Error, Debug)]
pub enum GtoError {
    #[error("Invalid rank: {0}")]
    InvalidRank(char),

    #[error("Invalid suit: {0}")]
    InvalidSuit(char),

    #[error("Invalid card notation: {0}")]
    InvalidCardNotation(String),

    #[error("Invalid board notation: {0}")]
    InvalidBoardNotation(String),

    #[error("Invalid hand notation: {0}")]
    InvalidHandNotation(String),

    #[error("Need at least {need} cards, got {got}")]
    NotEnoughCards { need: usize, got: usize },

    #[error("Cannot deal {requested} cards, only {available} remaining")]
    NotEnoughDeck { requested: usize, available: usize },

    #[error("Hand must be exactly 2 cards")]
    InvalidHandSize,

    #[error("Invalid value: {0}")]
    InvalidValue(String),

    #[error("No valid villain combos after removing dead cards")]
    NoValidCombos,

    #[error("No range data for: {0}")]
    RangeDataNotFound(String),

    #[error("JSON error: {0}")]
    JsonError(#[from] serde_json::Error),
}

pub type GtoResult<T> = Result<T, GtoError>;
