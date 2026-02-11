import pytest
from gto.cards import Card, parse_board
from gto.hand_evaluator import evaluate_hand, compare_hands, HandResult


def c(notation):
    return Card(notation[0], notation[1])


class TestEvaluateHand:
    def test_royal_flush(self):
        hole = [c("As"), c("Ks")]
        board = parse_board("QsTsJs2h3d")
        result = evaluate_hand(hole, board)
        assert result.category == "Royal Flush"
        assert result.rank == 9

    def test_straight_flush(self):
        hole = [c("9h"), c("8h")]
        board = parse_board("7h6h5hAcKd")
        result = evaluate_hand(hole, board)
        assert result.category == "Straight Flush"

    def test_four_of_a_kind(self):
        hole = [c("Ks"), c("Kh")]
        board = parse_board("KdKc5s2h3d")
        result = evaluate_hand(hole, board)
        assert result.category == "Four of a Kind"

    def test_full_house(self):
        hole = [c("As"), c("Ah")]
        board = parse_board("AdKsKh2c3d")
        result = evaluate_hand(hole, board)
        assert result.category == "Full House"
        assert result.kickers == (14, 13)

    def test_flush(self):
        hole = [c("As"), c("Ts")]
        board = parse_board("8s5s2sKdQh")
        result = evaluate_hand(hole, board)
        assert result.category == "Flush"

    def test_straight(self):
        hole = [c("9s"), c("8h")]
        board = parse_board("7d6c5sAhKd")
        result = evaluate_hand(hole, board)
        assert result.category == "Straight"
        assert result.kickers == (9,)

    def test_wheel(self):
        hole = [c("As"), c("2h")]
        board = parse_board("3d4c5sKhQd")
        result = evaluate_hand(hole, board)
        assert result.category == "Straight"
        assert result.kickers == (5,)

    def test_three_of_a_kind(self):
        hole = [c("Qs"), c("Qh")]
        board = parse_board("Qd7s3h2cKd")
        result = evaluate_hand(hole, board)
        assert result.category == "Three of a Kind"

    def test_two_pair(self):
        hole = [c("As"), c("Kh")]
        board = parse_board("AdKs5c2h3d")
        result = evaluate_hand(hole, board)
        assert result.category == "Two Pair"
        assert result.kickers == (14, 13, 5)

    def test_one_pair(self):
        hole = [c("As"), c("Ah")]
        board = parse_board("Kd7s3c2h5d")
        result = evaluate_hand(hole, board)
        assert result.category == "One Pair"
        assert result.kickers == (14, 13, 7, 5)

    def test_high_card(self):
        hole = [c("As"), c("Kh")]
        board = parse_board("Qd9s3c2h5d")
        result = evaluate_hand(hole, board)
        assert result.category == "High Card"

    def test_not_enough_cards(self):
        with pytest.raises(ValueError):
            evaluate_hand([c("As"), c("Kh")], [c("Qd")])


class TestCompareHands:
    def test_flush_beats_straight(self):
        board = parse_board("7s6s5s4dAh")
        assert compare_hands([c("As"), c("2s")], [c("8h"), c("9h")], board) == 1

    def test_higher_pair_wins(self):
        board = parse_board("2s5d8cTh3d")
        assert compare_hands([c("As"), c("Ah")], [c("Ks"), c("Kh")], board) == 1

    def test_kicker_decides(self):
        board = parse_board("As5d8cTh3d")
        assert compare_hands([c("Ad"), c("Kh")], [c("Ah"), c("Qd")], board) == 1

    def test_tie(self):
        board = parse_board("AsKdQhJsTs")
        assert compare_hands([c("2h"), c("3d")], [c("4h"), c("5d")], board) == 0

    def test_two_pair_kicker(self):
        board = parse_board("AsAd5s5d2c")
        r = compare_hands([c("Kh"), c("3c")], [c("Qh"), c("3d")], board)
        assert r == 1


class TestHandResultComparison:
    def test_ordering(self):
        high = HandResult(0, "High Card", (14, 13, 12, 11, 9), ())
        pair = HandResult(1, "One Pair", (14, 13, 12, 11), ())
        assert pair > high
        assert high < pair

    def test_same_rank_kicker(self):
        h1 = HandResult(1, "One Pair", (14, 13, 12, 11), ())
        h2 = HandResult(1, "One Pair", (14, 13, 12, 10), ())
        assert h1 > h2
