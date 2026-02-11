mod cards;
mod cli;
mod display;
mod equity;
mod error;
mod hand_evaluator;
mod math_engine;
mod multiway;
mod play;
mod postflop;
mod preflop;
mod ranges;

fn main() {
    cli::run();
}
