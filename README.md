# gto-cli

A comprehensive GTO poker toolkit for the command line. Covers all positions, all streets, all the math.

## Install

```bash
python -m venv .venv && source .venv/bin/activate
pip install -e .
```

Or just run directly:

```bash
python -m gto.cli <command>
```

## Commands

### `gto range` — Preflop Ranges

```bash
gto range BTN                              # BTN opening range
gto range CO --table 9max                  # CO range in 9-max
gto range BB --vs BTN --situation vs_RFI   # BB defense vs BTN open
gto range CO --vs UTG --situation vs_3bet  # CO facing UTG 3-bet
```

### `gto equity` — Equity Calculator

```bash
gto equity AhAs vs KsKd                   # Hand vs hand
gto equity AhAs vs KK                     # Hand vs range
gto equity AhAs vs KsKd --board AsKd5c    # With board
gto equity AhAs vs "QQ,JJ,TT" --sims 50000
```

### `gto odds` — Pot Odds & EV

```bash
gto odds 100 50                            # Pot odds
gto odds 100 50 --equity 0.35             # With EV calculation
gto odds 100 50 --implied 200             # With implied odds
```

### `gto board` — Board Texture Analysis

```bash
gto board AsKd7c                           # Dry rainbow flop
gto board Ts9s8d                           # Wet connected flop
gto board AsKdQhJs                         # Turn analysis
```

### `gto action` — Full Decision Advisor

```bash
gto action AKs -p BTN                     # Preflop RFI
gto action AKs -p CO --vs UTG -s vs_RFI   # Facing open

# Postflop with full context
gto action AKs -p BTN -b AsKd7c \
  --pot 100 --stack 500 --street flop --strength strong
```

### `gto mdf` — Minimum Defense Frequency

```bash
gto mdf 100 50                             # Heads-up
gto mdf 100 75 --players 3                # Multiway
```

### `gto spr` — Stack-to-Pot Ratio

```bash
gto spr 500 100                            # Medium SPR
gto spr 200 100                            # Low SPR
```

### `gto combos` — Combo Counter

```bash
gto combos "AA,KK,QQ,AKs"                 # Specific hands
gto combos "TT+"                           # Pairs TT and above
gto combos "ATs-AKs"                       # Suited range
```

### `gto bluff` — Bluff Math

```bash
gto bluff 100 75                           # 3/4 pot bet
gto bluff 100 100                          # Full pot bet
```

## Features

- Full 6-max and 9-max GTO preflop ranges (RFI, vs RFI, vs 3-bet, squeeze, BB defense)
- Monte Carlo equity calculator (hand vs hand, hand vs range)
- Complete poker math (pot odds, EV, MDF, fold equity, SPR, implied odds)
- Board texture analysis with c-bet recommendations
- Multiway pot adjustments
- Range grid visualization with color coding
- Clean, fast CLI powered by Click and Rich

## Dependencies

- Python 3.10+
- click
- rich
