import random
from dataclasses import dataclass
from typing import Optional

RANKS = "23456789TJQKA"
SUITS = "shdc"
RANK_VALUES = {r: i for i, r in enumerate(RANKS, 2)}
SUIT_NAMES = {"s": "spades", "h": "hearts", "d": "diamonds", "c": "clubs"}
SUIT_SYMBOLS = {"s": "\u2660", "h": "\u2665", "d": "\u2666", "c": "\u2663"}


@dataclass(frozen=True)
class Card:
    rank: str
    suit: str

    def __post_init__(self):
        if self.rank not in RANKS:
            raise ValueError(f"Invalid rank: {self.rank}")
        if self.suit not in SUITS:
            raise ValueError(f"Invalid suit: {self.suit}")

    @property
    def value(self) -> int:
        return RANK_VALUES[self.rank]

    def __str__(self) -> str:
        return f"{self.rank}{self.suit}"

    def __repr__(self) -> str:
        return f"Card('{self.rank}{self.suit}')"

    def pretty(self) -> str:
        return f"{self.rank}{SUIT_SYMBOLS[self.suit]}"

    def __lt__(self, other: "Card") -> bool:
        return self.value < other.value

    def __hash__(self) -> int:
        return hash((self.rank, self.suit))

    def __eq__(self, other: object) -> bool:
        if not isinstance(other, Card):
            return NotImplemented
        return self.rank == other.rank and self.suit == other.suit


def parse_card(notation: str) -> Card:
    notation = notation.strip()
    if len(notation) != 2:
        raise ValueError(f"Invalid card notation: {notation}")
    return Card(notation[0].upper(), notation[1].lower())


def parse_board(notation: str) -> list[Card]:
    notation = notation.strip().replace(" ", "").replace(",", "")
    if len(notation) % 2 != 0:
        raise ValueError(f"Invalid board notation: {notation}")
    return [parse_card(notation[i:i+2]) for i in range(0, len(notation), 2)]


class Deck:
    def __init__(self, exclude: Optional[list[Card]] = None):
        excluded = set(exclude) if exclude else set()
        self.cards = [Card(r, s) for r in RANKS for s in SUITS if Card(r, s) not in excluded]

    def shuffle(self):
        random.shuffle(self.cards)
        return self

    def deal(self, n: int = 1) -> list[Card]:
        if n > len(self.cards):
            raise ValueError(f"Cannot deal {n} cards, only {len(self.cards)} remaining")
        dealt = self.cards[:n]
        self.cards = self.cards[n:]
        return dealt

    def __len__(self) -> int:
        return len(self.cards)


def simplify_hand(cards: list[Card]) -> str:
    if len(cards) != 2:
        raise ValueError("Hand must be exactly 2 cards")
    c1, c2 = cards
    r1, r2 = c1.rank, c2.rank
    if RANK_VALUES[r1] < RANK_VALUES[r2]:
        r1, r2 = r2, r1
    if r1 == r2:
        return f"{r1}{r2}"
    suffix = "s" if c1.suit == c2.suit else "o"
    return f"{r1}{r2}{suffix}"


def hand_combos(notation: str) -> list[tuple[Card, Card]]:
    notation = notation.strip()
    if len(notation) == 2 and notation[0] == notation[1]:
        rank = notation[0]
        suits = list(SUITS)
        return [(Card(rank, s1), Card(rank, s2))
                for i, s1 in enumerate(suits) for s2 in suits[i+1:]]

    if len(notation) == 3:
        r1, r2, kind = notation[0], notation[1], notation[2]
        if kind == "s":
            return [(Card(r1, s), Card(r2, s)) for s in SUITS]
        elif kind == "o":
            return [(Card(r1, s1), Card(r2, s2))
                    for s1 in SUITS for s2 in SUITS if s1 != s2]

    if len(notation) == 4:
        return [(parse_card(notation[:2]), parse_card(notation[2:]))]

    raise ValueError(f"Invalid hand notation: {notation}")
