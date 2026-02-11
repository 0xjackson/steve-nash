import pytest
from gto.cards import Card, Deck, parse_card, parse_board, simplify_hand, hand_combos


class TestCard:
    def test_creation(self):
        c = Card("A", "s")
        assert c.rank == "A"
        assert c.suit == "s"
        assert c.value == 14

    def test_invalid_rank(self):
        with pytest.raises(ValueError):
            Card("X", "s")

    def test_invalid_suit(self):
        with pytest.raises(ValueError):
            Card("A", "x")

    def test_str(self):
        assert str(Card("K", "d")) == "Kd"

    def test_pretty(self):
        assert Card("A", "s").pretty() == "A\u2660"

    def test_ordering(self):
        assert Card("2", "s") < Card("A", "s")
        assert not Card("K", "h") < Card("Q", "d")

    def test_equality(self):
        assert Card("A", "s") == Card("A", "s")
        assert Card("A", "s") != Card("A", "h")

    def test_hashable(self):
        s = {Card("A", "s"), Card("A", "s"), Card("K", "h")}
        assert len(s) == 2


class TestParseCard:
    def test_basic(self):
        assert parse_card("As") == Card("A", "s")
        assert parse_card("Td") == Card("T", "d")

    def test_case_insensitive_suit(self):
        assert parse_card("AH") == Card("A", "h")

    def test_invalid(self):
        with pytest.raises(ValueError):
            parse_card("ABC")


class TestParseBoard:
    def test_flop(self):
        board = parse_board("AsKdQh")
        assert len(board) == 3
        assert board[0] == Card("A", "s")

    def test_with_spaces(self):
        board = parse_board("As Kd Qh")
        assert len(board) == 3

    def test_turn(self):
        board = parse_board("AsKdQh5c")
        assert len(board) == 4

    def test_river(self):
        board = parse_board("As Kd Qh 5c 2s")
        assert len(board) == 5


class TestDeck:
    def test_full_deck(self):
        d = Deck()
        assert len(d) == 52

    def test_exclude(self):
        excluded = [Card("A", "s"), Card("K", "h")]
        d = Deck(exclude=excluded)
        assert len(d) == 50

    def test_deal(self):
        d = Deck()
        cards = d.deal(5)
        assert len(cards) == 5
        assert len(d) == 47

    def test_deal_too_many(self):
        d = Deck()
        with pytest.raises(ValueError):
            d.deal(53)

    def test_shuffle(self):
        d = Deck()
        original = d.cards.copy()
        d.shuffle()
        assert len(d) == 52
        assert set(d.cards) == set(original)


class TestSimplifyHand:
    def test_pair(self):
        assert simplify_hand([Card("A", "s"), Card("A", "h")]) == "AA"

    def test_suited(self):
        assert simplify_hand([Card("A", "s"), Card("K", "s")]) == "AKs"

    def test_offsuit(self):
        assert simplify_hand([Card("A", "s"), Card("K", "h")]) == "AKo"

    def test_ordering(self):
        assert simplify_hand([Card("K", "s"), Card("A", "s")]) == "AKs"
        assert simplify_hand([Card("9", "h"), Card("T", "d")]) == "T9o"


class TestHandCombos:
    def test_pair(self):
        combos = hand_combos("AA")
        assert len(combos) == 6

    def test_suited(self):
        combos = hand_combos("AKs")
        assert len(combos) == 4
        for c1, c2 in combos:
            assert c1.suit == c2.suit

    def test_offsuit(self):
        combos = hand_combos("AKo")
        assert len(combos) == 12
        for c1, c2 in combos:
            assert c1.suit != c2.suit

    def test_specific(self):
        combos = hand_combos("AsKh")
        assert len(combos) == 1
        assert combos[0] == (Card("A", "s"), Card("K", "h"))
