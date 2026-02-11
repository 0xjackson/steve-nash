import pytest
from gto.preflop import (
    get_rfi_range, get_rfi_pct, get_vs_rfi_range, get_vs_3bet_range,
    get_squeeze_range, get_bb_defense, preflop_action, positions_for,
)


class TestGetRFIRange:
    def test_utg_6max(self):
        r = get_rfi_range("UTG", "6max")
        assert "AA" in r
        assert "KK" in r
        assert "72o" not in r

    def test_btn_has_more(self):
        utg = get_rfi_range("UTG", "6max")
        btn = get_rfi_range("BTN", "6max")
        assert len(btn) > len(utg)

    def test_9max(self):
        r = get_rfi_range("UTG", "9max")
        assert "AA" in r

    def test_open_pct(self):
        assert get_rfi_pct("UTG", "6max") > 0
        assert get_rfi_pct("BTN", "6max") > get_rfi_pct("UTG", "6max")


class TestGetVsRFI:
    def test_btn_vs_utg(self):
        result = get_vs_rfi_range("BTN", "UTG", "6max")
        assert "AA" in result.three_bet
        assert len(result.call) > 0

    def test_bb_vs_btn(self):
        result = get_vs_rfi_range("BB", "BTN", "6max")
        assert len(result.call) > 10  # BB defends wide vs BTN
        assert len(result.three_bet) > 0


class TestGetVs3Bet:
    def test_utg_vs_any(self):
        result = get_vs_3bet_range("UTG", "HJ", "6max")
        assert "AA" in result.four_bet
        assert "QQ" in result.call

    def test_btn_vs_sb(self):
        result = get_vs_3bet_range("BTN", "SB", "6max")
        assert len(result.call) > 5


class TestSqueeze:
    def test_sb_squeeze(self):
        r = get_squeeze_range("SB", "UTG", "HJ", "6max")
        assert "AA" in r


class TestBBDefense:
    def test_bb_vs_btn(self):
        r = get_bb_defense("BTN", "6max")
        assert len(r.call) > 20
        assert len(r.three_bet) > 5


class TestPreflopAction:
    def test_rfi_raise(self):
        result = preflop_action("AA", "UTG", "RFI", table_size="6max")
        assert result.action == "RAISE"

    def test_rfi_fold(self):
        result = preflop_action("72o", "UTG", "RFI", table_size="6max")
        assert result.action == "FOLD"

    def test_vs_rfi_3bet(self):
        result = preflop_action("AA", "BTN", "vs_RFI", villain_pos="UTG", table_size="6max")
        assert result.action == "3BET"

    def test_vs_rfi_call(self):
        result = preflop_action("AQs", "BTN", "vs_RFI", villain_pos="UTG", table_size="6max")
        assert result.action == "CALL"

    def test_vs_3bet_4bet(self):
        result = preflop_action("AA", "UTG", "vs_3bet", villain_pos="HJ", table_size="6max")
        assert result.action == "4BET"

    def test_missing_villain(self):
        with pytest.raises(ValueError):
            preflop_action("AA", "BTN", "vs_RFI", table_size="6max")


class TestPositions:
    def test_6max(self):
        assert len(positions_for("6max")) == 6

    def test_9max(self):
        assert len(positions_for("9max")) == 9
