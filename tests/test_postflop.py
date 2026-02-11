import pytest
from gto.cards import Card, parse_board
from gto.postflop import analyze_board, cbet_recommendation, bet_sizing, street_strategy


class TestAnalyzeBoard:
    def test_dry_rainbow(self):
        board = parse_board("Ks7d2c")
        result = analyze_board(board)
        assert result.is_rainbow
        assert result.wetness == "dry"
        assert result.high_card == "K"
        assert not result.is_paired

    def test_monotone(self):
        board = parse_board("Ts8s3s")
        result = analyze_board(board)
        assert result.is_monotone
        assert result.wetness == "wet"

    def test_paired(self):
        board = parse_board("Ks Kd 7c")
        result = analyze_board(board)
        assert result.is_paired

    def test_connected(self):
        board = parse_board("9s8d7c")
        result = analyze_board(board)
        assert result.connectedness == "connected"
        assert result.straight_draw_possible

    def test_two_tone(self):
        board = parse_board("AsKs7d")
        result = analyze_board(board)
        assert result.is_two_tone
        assert result.flush_draw_possible

    def test_turn(self):
        board = parse_board("As Kd Qh Js")
        result = analyze_board(board)
        assert len(result.cards) == 4

    def test_too_few_cards(self):
        with pytest.raises(ValueError):
            analyze_board(parse_board("AsKd"))


class TestCBetRecommendation:
    def test_dry_ip(self):
        board = parse_board("Ks7d2c")
        texture = analyze_board(board)
        rec = cbet_recommendation(texture, "IP")
        assert rec.should_cbet
        assert rec.frequency >= 0.6
        assert "33%" in rec.sizing

    def test_wet_oop(self):
        board = parse_board("Ts9s8d")
        texture = analyze_board(board)
        rec = cbet_recommendation(texture, "OOP")
        assert rec.frequency < 0.5

    def test_multiway(self):
        board = parse_board("Ks7d2c")
        texture = analyze_board(board)
        rec = cbet_recommendation(texture, "IP", multiway=True)
        assert rec.frequency < 0.5


class TestBetSizing:
    def test_dry_flop(self):
        board = parse_board("Ks7d2c")
        texture = analyze_board(board)
        size = bet_sizing(texture, 8.0, "flop")
        assert "25" in size or "33" in size

    def test_wet_flop(self):
        board = parse_board("Ts9s8d")
        texture = analyze_board(board)
        size = bet_sizing(texture, 8.0, "flop")
        assert "66" in size or "75" in size

    def test_low_spr(self):
        board = parse_board("Ks7d2c")
        texture = analyze_board(board)
        size = bet_sizing(texture, 2.0, "flop")
        assert "low SPR" in size.lower() or "commit" in size.lower()

    def test_polarized(self):
        board = parse_board("Ks7d2c")
        texture = analyze_board(board)
        size = bet_sizing(texture, 8.0, "river", polarized=True)
        assert "75" in size or "125" in size


class TestStreetStrategy:
    def test_nuts_bet(self):
        board = parse_board("Ks7d2c")
        texture = analyze_board(board)
        result = street_strategy("nuts", texture, 100, 500, "IP", "flop")
        assert result.action == "BET"

    def test_medium_check_oop(self):
        board = parse_board("Ks7d2c")
        texture = analyze_board(board)
        result = street_strategy("medium", texture, 100, 500, "OOP", "flop")
        assert "CHECK" in result.action

    def test_draw_semi_bluff(self):
        board = parse_board("Ts9s8d")
        texture = analyze_board(board)
        result = street_strategy("draw", texture, 100, 500, "IP", "flop")
        assert "bluff" in result.action.lower() or "BET" in result.action

    def test_weak_fold(self):
        board = parse_board("Ks7d2c")
        texture = analyze_board(board)
        result = street_strategy("weak", texture, 100, 500, "OOP", "flop")
        assert "FOLD" in result.action or "CHECK" in result.action
