mod card_encoding;
mod cards;
mod cfr;
mod cli;
mod display;
mod equity;
mod error;
mod game_tree;
mod hand_evaluator;
mod lookup_eval;
mod math_engine;
mod multiway;
mod play;
mod postflop;
mod preflop;
mod preflop_solver;
mod ranges;

fn main() {
    cli::run();
}
