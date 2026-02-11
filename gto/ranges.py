from gto.cards import Card, RANKS, SUITS, RANK_VALUES, hand_combos

ALL_PAIRS = [f"{r}{r}" for r in RANKS]
ALL_SUITED = [f"{RANKS[i]}{RANKS[j]}s" for i in range(len(RANKS)-1, -1, -1)
              for j in range(i-1, -1, -1)]
ALL_OFFSUIT = [f"{RANKS[i]}{RANKS[j]}o" for i in range(len(RANKS)-1, -1, -1)
               for j in range(i-1, -1, -1)]

# Ordered by approximate preflop equity (strongest first)
HAND_RANKING = [
    "AA", "KK", "QQ", "AKs", "JJ", "AQs", "KQs", "AJs", "KJs", "TT",
    "AKo", "ATs", "QJs", "KTs", "QTs", "JTs", "99", "AQo", "A9s", "KQo",
    "K9s", "T9s", "J9s", "Q9s", "A8s", "88", "A5s", "A7s", "A4s", "A6s",
    "A3s", "K8s", "T8s", "A2s", "98s", "J8s", "77", "Q8s", "K7s", "AJo",
    "87s", "66", "K6s", "ATu", "97s", "76s", "T7s", "K5s", "ATo", "55",
    "J7s", "86s", "KJo", "65s", "Q7s", "K4s", "K3s", "K2s", "96s", "44",
    "QJo", "75s", "54s", "A9o", "T6s", "KTo", "J6s", "Q6s", "33", "85s",
    "64s", "QTo", "22", "53s", "JTo", "K9o", "J9o", "T9o", "Q9o", "74s",
    "43s", "A8o", "A5o", "A7o", "A4o", "A6o", "A3o", "95s", "63s", "A2o",
    "52s", "84s", "42s", "T8o", "98o", "J8o", "Q8o", "73s", "87o", "32s",
    "62s", "97o", "76o", "K8o", "86o", "65o", "94s", "93s", "92s", "T7o",
    "54o", "83s", "75o", "82s", "K7o", "K6o", "72s", "96o", "J7o", "K5o",
    "T6o", "K4o", "K3o", "K2o", "85o", "Q7o", "64o", "53o", "J6o", "Q6o",
    "Q5o", "Q4o", "Q3o", "Q2o", "74o", "43o", "95o", "63o", "84o", "42o",
    "T5o", "T4o", "T3o", "T2o", "52o", "J5o", "J4o", "J3o", "J2o", "73o",
    "32o", "62o", "94o", "93o", "92o", "83o", "82o", "72o",
]

# Fix: "ATu" should be "ATo" — some notation uses 'u' for unsuited
HAND_RANKING = [h.replace("ATu", "ATo") for h in HAND_RANKING]


def combo_count(notation: str) -> int:
    if len(notation) == 2 and notation[0] == notation[1]:
        return 6
    if len(notation) == 3:
        if notation[2] == "s":
            return 4
        if notation[2] == "o":
            return 12
    return 0


def parse_range(range_str: str) -> list[str]:
    hands = set()
    for part in range_str.replace(" ", "").split(","):
        part = part.strip()
        if not part:
            continue
        if part.endswith("+"):
            hands.update(_expand_plus(part[:-1]))
        elif "-" in part and len(part) > 3:
            hands.update(_expand_dash(part))
        else:
            hands.add(part)
    return sorted(hands, key=lambda h: _hand_strength_index(h))


def _expand_plus(base: str) -> list[str]:
    if len(base) == 2 and base[0] == base[1]:
        rank_idx = RANKS.index(base[0])
        return [f"{r}{r}" for r in RANKS[rank_idx:]]

    if len(base) == 3:
        high, low, kind = base[0], base[1], base[2]
        low_idx = RANKS.index(low)
        high_idx = RANKS.index(high)
        return [f"{high}{RANKS[i]}{kind}" for i in range(low_idx, high_idx)]

    return [base]


def _expand_dash(range_str: str) -> list[str]:
    parts = range_str.split("-")
    if len(parts) != 2:
        return [range_str]

    start, end = parts
    if len(start) == 2 and len(end) == 2 and start[0] == start[1] and end[0] == end[1]:
        lo = min(RANKS.index(start[0]), RANKS.index(end[0]))
        hi = max(RANKS.index(start[0]), RANKS.index(end[0]))
        return [f"{RANKS[i]}{RANKS[i]}" for i in range(lo, hi + 1)]

    if len(start) == 3 and len(end) == 3 and start[0] == end[0] and start[2] == end[2]:
        high = start[0]
        kind = start[2]
        lo = min(RANKS.index(start[1]), RANKS.index(end[1]))
        hi = max(RANKS.index(start[1]), RANKS.index(end[1]))
        return [f"{high}{RANKS[i]}{kind}" for i in range(lo, hi + 1)]

    return [range_str]


def _hand_strength_index(hand: str) -> int:
    try:
        return HAND_RANKING.index(hand)
    except ValueError:
        return len(HAND_RANKING)


def range_from_top_pct(pct: float) -> list[str]:
    if pct <= 0 or pct > 100:
        raise ValueError("Percentage must be between 0 and 100")
    total_combos = 1326
    target = total_combos * (pct / 100)
    result = []
    running = 0
    for hand in HAND_RANKING:
        count = combo_count(hand)
        if running + count > target and running > 0:
            break
        result.append(hand)
        running += count
        if running >= target:
            break
    return result


def total_combos(hands: list[str]) -> int:
    return sum(combo_count(h) for h in hands)


def range_pct(hands: list[str]) -> float:
    return total_combos(hands) / 1326 * 100


def blockers_remove(villain_range: list[str], hero_cards: list[Card]) -> list[str]:
    result = []
    for hand in villain_range:
        combos = hand_combos(hand)
        remaining = [(c1, c2) for c1, c2 in combos
                     if c1 not in hero_cards and c2 not in hero_cards]
        if remaining:
            result.append(hand)
    return result


def blocked_combos(hand_notation: str, hero_cards: list[Card]) -> int:
    combos = hand_combos(hand_notation)
    remaining = [(c1, c2) for c1, c2 in combos
                 if c1 not in hero_cards and c2 not in hero_cards]
    return combo_count(hand_notation) - len(remaining)
