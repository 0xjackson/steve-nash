from collections import Counter
from dataclasses import dataclass
from itertools import combinations
from typing import Optional

from gto.cards import Card, RANK_VALUES

HAND_RANKS = {
    "Royal Flush": 9,
    "Straight Flush": 8,
    "Four of a Kind": 7,
    "Full House": 6,
    "Flush": 5,
    "Straight": 4,
    "Three of a Kind": 3,
    "Two Pair": 2,
    "One Pair": 1,
    "High Card": 0,
}


@dataclass
class HandResult:
    rank: int
    category: str
    kickers: tuple[int, ...]
    cards: tuple[Card, ...]

    def __lt__(self, other: "HandResult") -> bool:
        if self.rank != other.rank:
            return self.rank < other.rank
        return self.kickers < other.kickers

    def __eq__(self, other: object) -> bool:
        if not isinstance(other, HandResult):
            return NotImplemented
        return self.rank == other.rank and self.kickers == other.kickers

    def __le__(self, other: "HandResult") -> bool:
        return self == other or self < other

    def __gt__(self, other: "HandResult") -> bool:
        return not self <= other

    def __ge__(self, other: "HandResult") -> bool:
        return not self < other

    def __str__(self) -> str:
        return self.category


def _is_flush(cards: list[Card]) -> bool:
    return len(set(c.suit for c in cards)) == 1


def _is_straight(values: list[int]) -> Optional[int]:
    v = sorted(set(values), reverse=True)
    if len(v) < 5:
        return None
    if v[0] - v[4] == 4 and len(v) == 5:
        return v[0]
    if set(v) == {14, 2, 3, 4, 5}:
        return 5  # wheel, 5-high straight
    return None


def _evaluate_five(cards: tuple[Card, ...]) -> HandResult:
    values = sorted([c.value for c in cards], reverse=True)
    counts = Counter(c.value for c in cards)
    flush = _is_flush(list(cards))
    straight_high = _is_straight(values)

    if flush and straight_high:
        if straight_high == 14:
            return HandResult(9, "Royal Flush", (14,), cards)
        return HandResult(8, "Straight Flush", (straight_high,), cards)

    freq = sorted(counts.items(), key=lambda x: (x[1], x[0]), reverse=True)

    if freq[0][1] == 4:
        quad_val = freq[0][0]
        kicker = max(v for v in values if v != quad_val)
        return HandResult(7, "Four of a Kind", (quad_val, kicker), cards)

    if freq[0][1] == 3 and freq[1][1] == 2:
        return HandResult(6, "Full House", (freq[0][0], freq[1][0]), cards)

    if flush:
        return HandResult(5, "Flush", tuple(values), cards)

    if straight_high:
        return HandResult(4, "Straight", (straight_high,), cards)

    if freq[0][1] == 3:
        trip_val = freq[0][0]
        kicks = sorted([v for v in values if v != trip_val], reverse=True)
        return HandResult(3, "Three of a Kind", (trip_val, *kicks), cards)

    pair_vals = sorted([v for v, c in counts.items() if c == 2], reverse=True)
    if len(pair_vals) == 2:
        kicker = max(v for v in values if v not in pair_vals)
        return HandResult(2, "Two Pair", (pair_vals[0], pair_vals[1], kicker), cards)

    if len(pair_vals) == 1:
        kicks = sorted([v for v in values if v != pair_vals[0]], reverse=True)
        return HandResult(1, "One Pair", (pair_vals[0], *kicks), cards)

    return HandResult(0, "High Card", tuple(values), cards)


def evaluate_hand(hole_cards: list[Card], board: list[Card]) -> HandResult:
    all_cards = hole_cards + board
    if len(all_cards) < 5:
        raise ValueError(f"Need at least 5 cards, got {len(all_cards)}")

    best = None
    for combo in combinations(all_cards, 5):
        result = _evaluate_five(combo)
        if best is None or result > best:
            best = result
    return best


def compare_hands(hand1: list[Card], hand2: list[Card], board: list[Card]) -> int:
    r1 = evaluate_hand(hand1, board)
    r2 = evaluate_hand(hand2, board)
    if r1 > r2:
        return 1
    if r1 < r2:
        return -1
    return 0
