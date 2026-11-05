import pytest

from linewright.commands.parse import parse_number, parse_point


@pytest.mark.parametrize(
    "text, base, expected",
    [
        ("10,20", None, (10, 20)),
        (" 10 , -20.5 ", None, (10, -20.5)),
        ("@5,5", (10, 10), (15, 15)),
        ("@5,5", None, (5, 5)),
        ("@10<90", (1, 1), (1, 11)),
        ("10<0", (99, 99), (10, 0)),
        ("@2<180", (0, 0), (-2, 0)),
    ],
)
def test_parse_point(text, base, expected):
    assert parse_point(text, base) == pytest.approx(expected, abs=1e-9)


@pytest.mark.parametrize("text", ["", "abc", "1", "1,2,3", "1,", "@", "5<", "nan,1", "inf,0", "1,2<3"])
def test_parse_point_rejects_garbage(text):
    assert parse_point(text, (0, 0)) is None


@pytest.mark.parametrize("text, expected", [("5", 5.0), ("0.25", 0.25), (" 3 ", 3.0)])
def test_parse_number(text, expected):
    assert parse_number(text) == expected


@pytest.mark.parametrize("text", ["0", "-1", "abc", "nan", "inf", "1,5", ""])
def test_parse_number_rejects_non_positive_and_garbage(text):
    assert parse_number(text) is None
