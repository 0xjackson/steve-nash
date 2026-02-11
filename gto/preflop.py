import json
from dataclasses import dataclass
from pathlib import Path
from typing import Optional

DATA_DIR = Path(__file__).parent.parent / "data"

_cache: dict[str, dict] = {}


def _load_ranges(table_size: str) -> dict:
    if table_size in _cache:
        return _cache[table_size]
    filename = f"ranges_{table_size}.json"
    path = DATA_DIR / filename
    if not path.exists():
        raise ValueError(f"No range data for table size: {table_size}")
    with open(path) as f:
        data = json.load(f)
    _cache[table_size] = data[table_size]
    return _cache[table_size]


POSITIONS_6MAX = ["UTG", "HJ", "CO", "BTN", "SB", "BB"]
POSITIONS_9MAX = ["UTG", "UTG1", "UTG2", "MP", "HJ", "CO", "BTN", "SB", "BB"]


def positions_for(table_size: str) -> list[str]:
    if table_size == "6max":
        return POSITIONS_6MAX
    return POSITIONS_9MAX


def get_rfi_range(position: str, table_size: str = "6max") -> list[str]:
    data = _load_ranges(table_size)
    rfi = data.get("RFI", {}).get(position, {})
    return rfi.get("raise", [])


def get_rfi_pct(position: str, table_size: str = "6max") -> int:
    data = _load_ranges(table_size)
    rfi = data.get("RFI", {}).get(position, {})
    return rfi.get("open_pct", 0)


@dataclass
class VsRFIResult:
    call: list[str]
    three_bet: list[str]
    fold: str

    @property
    def action_for(self):
        return self


def get_vs_rfi_range(hero_pos: str, villain_pos: str, table_size: str = "6max") -> VsRFIResult:
    data = _load_ranges(table_size)
    key = f"{hero_pos}_vs_{villain_pos}"
    vs_rfi = data.get("vs_RFI", {}).get(key, {})
    if not vs_rfi:
        # Try BB defense as fallback
        if hero_pos == "BB":
            bb_def = data.get("bb_defense", {}).get(f"vs_{villain_pos}", {})
            if bb_def:
                return VsRFIResult(
                    call=bb_def.get("call", []),
                    three_bet=bb_def.get("3bet", []),
                    fold="default",
                )
        return VsRFIResult(call=[], three_bet=[], fold="default")
    return VsRFIResult(
        call=vs_rfi.get("call", []),
        three_bet=vs_rfi.get("3bet", []),
        fold=vs_rfi.get("fold", "default"),
    )


@dataclass
class Vs3BetResult:
    call: list[str]
    four_bet: list[str]
    fold: str


def get_vs_3bet_range(hero_pos: str, villain_pos: str, table_size: str = "6max") -> Vs3BetResult:
    data = _load_ranges(table_size)
    key = f"{hero_pos}_vs_{villain_pos}"
    vs_3bet = data.get("vs_3bet", {}).get(key, {})
    if not vs_3bet:
        # Try generic fallback
        generic = data.get("vs_3bet", {}).get(f"{hero_pos}_vs_any", {})
        if generic:
            vs_3bet = generic
        else:
            return Vs3BetResult(call=[], four_bet=[], fold="default")
    return Vs3BetResult(
        call=vs_3bet.get("call", []),
        four_bet=vs_3bet.get("4bet", []),
        fold=vs_3bet.get("fold", "default"),
    )


def get_squeeze_range(hero_pos: str, raiser_pos: str, caller_pos: str, table_size: str = "6max") -> list[str]:
    data = _load_ranges(table_size)
    squeeze = data.get("squeeze", {})
    key = f"{hero_pos}_vs_{raiser_pos}_{caller_pos}"
    if key in squeeze:
        return squeeze[key].get("squeeze", [])
    for k, v in squeeze.items():
        if k.startswith(f"{hero_pos}_vs_"):
            return v.get("squeeze", [])
    return []


def get_bb_defense(vs_position: str, table_size: str = "6max") -> VsRFIResult:
    data = _load_ranges(table_size)
    bb_def = data.get("bb_defense", {}).get(f"vs_{vs_position}", {})
    return VsRFIResult(
        call=bb_def.get("call", []),
        three_bet=bb_def.get("3bet", []),
        fold="default",
    )


@dataclass
class PreflopAction:
    action: str
    hand: str
    position: str
    detail: str


def preflop_action(
    hand: str,
    position: str,
    situation: str = "RFI",
    villain_pos: Optional[str] = None,
    table_size: str = "6max",
) -> PreflopAction:
    if situation == "RFI":
        rfi = get_rfi_range(position, table_size)
        if hand in rfi:
            return PreflopAction("RAISE", hand, position,
                                 f"Open raise from {position}")
        return PreflopAction("FOLD", hand, position,
                             f"Not in {position} opening range")

    if situation == "vs_RFI":
        if not villain_pos:
            raise ValueError("villain_pos required for vs_RFI")
        result = get_vs_rfi_range(position, villain_pos, table_size)
        if hand in result.three_bet:
            return PreflopAction("3BET", hand, position,
                                 f"3-bet vs {villain_pos} open")
        if hand in result.call:
            return PreflopAction("CALL", hand, position,
                                 f"Call {villain_pos} open")
        return PreflopAction("FOLD", hand, position,
                             f"Fold vs {villain_pos} open")

    if situation == "vs_3bet":
        if not villain_pos:
            raise ValueError("villain_pos required for vs_3bet")
        result = get_vs_3bet_range(position, villain_pos, table_size)
        if hand in result.four_bet:
            return PreflopAction("4BET", hand, position,
                                 f"4-bet vs {villain_pos} 3-bet")
        if hand in result.call:
            return PreflopAction("CALL", hand, position,
                                 f"Call {villain_pos} 3-bet")
        return PreflopAction("FOLD", hand, position,
                             f"Fold vs {villain_pos} 3-bet")

    return PreflopAction("FOLD", hand, position, "Unknown situation")
