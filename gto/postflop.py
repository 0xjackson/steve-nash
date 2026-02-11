from dataclasses import dataclass, field
from gto.cards import Card, RANK_VALUES


@dataclass
class BoardTexture:
    cards: list[Card]
    high_card: str
    is_paired: bool
    is_monotone: bool
    is_two_tone: bool
    is_rainbow: bool
    flush_draw_possible: bool
    straight_draw_possible: bool
    connectedness: str  # "disconnected", "semi-connected", "connected"
    wetness: str  # "dry", "medium", "wet"
    category: str  # human-readable summary
    draws: list[str] = field(default_factory=list)


def analyze_board(board_cards: list[Card]) -> BoardTexture:
    if len(board_cards) < 3:
        raise ValueError("Need at least 3 board cards")

    values = sorted([c.value for c in board_cards], reverse=True)
    suits = [c.suit for c in board_cards]
    suit_counts = {}
    for s in suits:
        suit_counts[s] = suit_counts.get(s, 0) + 1
    max_suit = max(suit_counts.values())

    is_monotone = max_suit >= 3 and len(set(suits[:3])) == 1
    is_two_tone = not is_monotone and max_suit >= 2
    is_rainbow = max_suit == 1

    rank_counts = {}
    for v in values:
        rank_counts[v] = rank_counts.get(v, 0) + 1
    is_paired = max(rank_counts.values()) >= 2

    gaps = []
    unique_vals = sorted(set(values))
    for i in range(len(unique_vals) - 1):
        gaps.append(unique_vals[i+1] - unique_vals[i])

    has_connected = any(g == 1 for g in gaps)
    has_one_gap = any(g == 2 for g in gaps)

    straight_draw = _has_straight_draw(values)
    flush_draw = max_suit >= 2 and len(board_cards) < 5

    if has_connected and sum(1 for g in gaps if g <= 2) >= 2:
        connectedness = "connected"
    elif has_connected or has_one_gap:
        connectedness = "semi-connected"
    else:
        connectedness = "disconnected"

    wet_score = 0
    if is_monotone:
        wet_score += 3
    elif is_two_tone:
        wet_score += 1
    if connectedness == "connected":
        wet_score += 2
    elif connectedness == "semi-connected":
        wet_score += 1
    if is_paired:
        wet_score -= 1

    if wet_score >= 3:
        wetness = "wet"
    elif wet_score >= 1:
        wetness = "medium"
    else:
        wetness = "dry"

    draws = []
    if flush_draw and is_two_tone:
        draws.append("flush draw")
    if is_monotone:
        draws.append("flush complete / 4-flush")
    if straight_draw:
        draws.append("straight draw")
    if is_paired:
        draws.append("paired board")

    high_rank = _value_to_rank(values[0])
    parts = []
    if is_monotone:
        parts.append("monotone")
    elif is_two_tone:
        parts.append("two-tone")
    else:
        parts.append("rainbow")
    parts.append(connectedness)
    if is_paired:
        parts.append("paired")
    parts.append(f"{high_rank}-high")
    category = " ".join(parts)

    return BoardTexture(
        cards=board_cards,
        high_card=high_rank,
        is_paired=is_paired,
        is_monotone=is_monotone,
        is_two_tone=is_two_tone,
        is_rainbow=is_rainbow,
        flush_draw_possible=flush_draw,
        straight_draw_possible=straight_draw,
        connectedness=connectedness,
        wetness=wetness,
        category=category,
        draws=draws,
    )


def _has_straight_draw(values: list[int]) -> bool:
    unique = sorted(set(values))
    # Check for 3+ cards within a 5-card window
    for i in range(len(unique)):
        window = [v for v in unique if v >= unique[i] and v <= unique[i] + 4]
        if len(window) >= 3:
            return True
    # Ace-low potential
    if 14 in unique:
        low_window = [v for v in unique if v <= 5] + [1]  # ace as 1
        if len(low_window) >= 3:
            return True
    return False


def _value_to_rank(value: int) -> str:
    for r, v in RANK_VALUES.items():
        if v == value:
            return r
    return "?"


@dataclass
class CBetRecommendation:
    should_cbet: bool
    frequency: float
    sizing: str
    reasoning: str


def cbet_recommendation(
    board_texture: BoardTexture,
    position: str = "IP",
    spr_value: float = 5.0,
    multiway: bool = False,
) -> CBetRecommendation:
    if multiway:
        if board_texture.wetness == "dry":
            return CBetRecommendation(True, 0.4, "33% pot",
                                      "Dry board multiway — small sizing, lower frequency")
        return CBetRecommendation(False, 0.2, "50% pot",
                                  "Wet board multiway — check most hands, bet selectively")

    ip = position.upper() == "IP"

    if board_texture.wetness == "dry":
        if ip:
            return CBetRecommendation(True, 0.7, "33% pot",
                                      "Dry board IP — high frequency small c-bet")
        return CBetRecommendation(True, 0.5, "33% pot",
                                  "Dry board OOP — moderate frequency small c-bet")

    if board_texture.wetness == "wet":
        if ip:
            return CBetRecommendation(True, 0.5, "66-75% pot",
                                      "Wet board IP — polarized sizing, moderate frequency")
        return CBetRecommendation(True, 0.35, "66-75% pot",
                                  "Wet board OOP — selective, larger sizing")

    # medium
    if ip:
        return CBetRecommendation(True, 0.6, "50% pot",
                                  "Medium texture IP — balanced frequency and sizing")
    return CBetRecommendation(True, 0.45, "50% pot",
                              "Medium texture OOP — moderate sizing")


def bet_sizing(
    board_texture: BoardTexture,
    spr_value: float,
    street: str = "flop",
    polarized: bool = False,
) -> str:
    if polarized:
        if street == "river":
            return "75-125% pot"
        return "66-75% pot"

    if spr_value <= 4:
        return "33-50% pot (low SPR — pot commitment)"

    if street == "flop":
        if board_texture.wetness == "dry":
            return "25-33% pot"
        if board_texture.wetness == "wet":
            return "66-75% pot"
        return "50% pot"

    if street == "turn":
        if board_texture.wetness == "dry":
            return "50% pot"
        return "66-75% pot"

    # river
    return "66-75% pot"


@dataclass
class StreetStrategy:
    action: str
    sizing: str
    reasoning: str
    hand_strength: str


def street_strategy(
    hand_strength: str,
    board_texture: BoardTexture,
    pot: float,
    stack: float,
    position: str = "IP",
    street: str = "flop",
) -> StreetStrategy:
    spr_val = stack / pot if pot > 0 else 10

    if hand_strength in ("nuts", "very_strong"):
        if spr_val <= 4:
            return StreetStrategy("BET", "all-in or 66-100% pot",
                                  "Low SPR with strong hand — build pot for stacks",
                                  hand_strength)
        sizing = bet_sizing(board_texture, spr_val, street, polarized=True)
        return StreetStrategy("BET", sizing, "Strong hand — value bet", hand_strength)

    if hand_strength == "strong":
        if board_texture.wetness == "wet":
            return StreetStrategy("BET", bet_sizing(board_texture, spr_val, street),
                                  "Strong hand on wet board — protect equity", hand_strength)
        return StreetStrategy("BET", bet_sizing(board_texture, spr_val, street),
                              "Strong hand — standard value", hand_strength)

    if hand_strength == "medium":
        if position.upper() == "IP":
            return StreetStrategy("CHECK/BET", "50% pot if betting",
                                  "Medium hand IP — pot control or thin value", hand_strength)
        return StreetStrategy("CHECK", "-",
                              "Medium hand OOP — pot control", hand_strength)

    if hand_strength == "draw":
        if board_texture.wetness == "wet" and position.upper() == "IP":
            return StreetStrategy("BET (semi-bluff)", bet_sizing(board_texture, spr_val, street),
                                  "Draw IP — semi-bluff for fold equity + equity", hand_strength)
        return StreetStrategy("CHECK/CALL", "-",
                              "Draw — realize equity cheaply", hand_strength)

    if hand_strength == "bluff":
        freq = 1 - (pot / (pot + stack)) if stack > 0 else 0.3
        return StreetStrategy("BET (bluff)", bet_sizing(board_texture, spr_val, street, polarized=True),
                              f"Bluff — need ~{freq:.0%} fold equity to profit", hand_strength)

    return StreetStrategy("CHECK/FOLD", "-",
                          "Weak hand — give up without equity", hand_strength)
