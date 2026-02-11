import pytest
from gto.cards import Card
from gto.ranges import (
    combo_count, parse_range, range_from_top_pct, total_combos,
    range_pct, blockers_remove, blocked_combos,
)


class TestComboCount:
    def test_pair(self):
        assert combo_count("AA") == 6

    def test_suited(self):
        assert combo_count("AKs") == 4

    def test_offsuit(self):
        assert combo_count("AKo") == 12


class TestParseRange:
    def test_simple(self):
        result = parse_range("AA,KK,QQ")
        assert "AA" in result
        assert "KK" in result
        assert "QQ" in result

    def test_plus_pairs(self):
        result = parse_range("TT+")
        assert "TT" in result
        assert "JJ" in result
        assert "QQ" in result
        assert "KK" in result
        assert "AA" in result
        assert "99" not in result

    def test_plus_suited(self):
        result = parse_range("ATs+")
        assert "ATs" in result
        assert "AJs" in result
        assert "AQs" in result
        assert "AKs" in result
        assert "A9s" not in result

    def test_dash_pairs(self):
        result = parse_range("77-TT")
        assert "77" in result
        assert "88" in result
        assert "99" in result
        assert "TT" in result
        assert "66" not in result
        assert "JJ" not in result

    def test_dash_suited(self):
        result = parse_range("KTs-KQs")
        assert "KTs" in result
        assert "KJs" in result
        assert "KQs" in result
        assert "K9s" not in result

    def test_mixed(self):
        result = parse_range("AA, KK, AKs, AQs+")
        assert "AA" in result
        assert "AKs" in result


class TestRangeFromTopPct:
    def test_top_1(self):
        result = range_from_top_pct(1)
        assert "AA" in result
        assert len(result) <= 3

    def test_top_10(self):
        result = range_from_top_pct(10)
        combos = total_combos(result)
        assert combos <= 1326 * 0.12

    def test_top_50(self):
        result = range_from_top_pct(50)
        assert len(result) > 20

    def test_invalid(self):
        with pytest.raises(ValueError):
            range_from_top_pct(0)
        with pytest.raises(ValueError):
            range_from_top_pct(101)


class TestTotalCombos:
    def test_basic(self):
        assert total_combos(["AA"]) == 6
        assert total_combos(["AA", "KK"]) == 12
        assert total_combos(["AKs"]) == 4
        assert total_combos(["AKo"]) == 12


class TestRangePct:
    def test_all_pairs(self):
        pairs = ["AA", "KK", "QQ", "JJ", "TT", "99", "88", "77", "66", "55", "44", "33", "22"]
        pct = range_pct(pairs)
        assert abs(pct - (78 / 1326 * 100)) < 0.1


class TestBlockers:
    def test_remove_blocked(self):
        hero = [Card("A", "s"), Card("K", "h")]
        result = blockers_remove(["AA", "KK", "QQ"], hero)
        assert "AA" in result  # still has some combos
        assert "QQ" in result  # not blocked at all

    def test_blocked_count(self):
        hero = [Card("A", "s"), Card("A", "h")]
        count = blocked_combos("AA", hero)
        assert count == 5  # holding 2 aces blocks 5 of 6 combos
