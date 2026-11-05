import pytest

from linewright.commands import curves, draw
from linewright.commands.registry import DRAWING, resolve
from linewright.commands.session import INVALID_ANSWER
from linewright.model.entities import Arc, Circle, Line, Polyline, Text
from helpers import new_session


def start(command):
    """A session for a drawing command. Returns the list its entities are added to."""
    host, session = new_session(command)
    return host.added, session


def run(command, *answers):
    """Drive a command with typed answers. Returns (entities committed, session)."""
    committed, session = start(command)
    for answer in answers:
        assert session.active, f"command ended early before {answer!r}"
        error = session.submit_text(answer)
        assert error is None, error
    return committed, session


class TestLine:
    def test_draws_a_segment_per_point_and_ends_on_enter(self):
        drawn, session = run(draw.line, "0,0", "10,0", "@0,5", "")
        assert drawn == [Line((0, 0), (10, 0)), Line((10, 0), (10, 5))]
        assert not session.active

    def test_close_joins_back_to_the_first_point_only_after_two_segments(self):
        _, session = run(draw.line, "0,0", "10,0")
        assert session.submit_text("c") == INVALID_ANSWER["point"]
        drawn, session = run(draw.line, "0,0", "10,0", "10,10", "c")
        assert drawn[-1] == Line((10, 10), (0, 0))
        assert not session.active

    def test_prompt_names_the_options_that_apply(self):
        _, session = run(draw.line, "0,0")
        assert session.prompt.message == "Specify next point or [Undo]:"
        _, session = run(draw.line, "0,0", "1,0", "2,0")
        assert session.prompt.message == "Specify next point or [Close/Undo]:"

    def test_escape_keeps_segments_already_drawn(self):
        drawn, session = run(draw.line, "0,0", "10,0")
        session.cancel()
        assert drawn == [Line((0, 0), (10, 0))]
        assert not session.active

    def test_undo_takes_back_the_last_segment_and_carries_on_from_the_one_before(self):
        drawn, session = run(draw.line, "0,0", "10,0", "10,10", "u")
        assert drawn == [Line((0, 0), (10, 0))]
        session.submit_text("20,0")
        assert drawn == [Line((0, 0), (10, 0)), Line((10, 0), (20, 0))]

    def test_undo_before_any_segment_asks_for_the_first_point_again(self):
        drawn, session = run(draw.line, "0,0", "u")
        assert drawn == []
        assert session.prompt.message == "Specify first point:"
        session.submit_text("5,5")
        session.submit_text("6,6")
        assert drawn == [Line((5, 5), (6, 6))]

    def test_undo_all_the_way_back(self):
        drawn, session = run(draw.line, "0,0", "1,0", "2,0", "u", "u", "u")
        assert drawn == []
        assert session.prompt.message == "Specify first point:"


class TestPolyline:
    def test_is_one_entity_and_escape_finishes_it(self):
        drawn, session = run(draw.polyline, "0,0", "10,0", "10,10")
        assert drawn == []
        session.cancel()
        assert drawn == [Polyline([(0, 0, 0.0), (10, 0, 0.0), (10, 10, 0.0)])]

    def test_close_and_too_few_points(self):
        drawn, _ = run(draw.polyline, "0,0", "10,0", "10,10", "c")
        assert drawn[0].closed
        drawn, _ = run(draw.polyline, "0,0", "")
        assert drawn == []

    def test_undo_removes_the_last_point(self):
        drawn, _ = run(draw.polyline, "0,0", "10,0", "10,10", "u", "20,0", "")
        assert drawn == [Polyline([(0, 0, 0.0), (10, 0, 0.0), (20, 0, 0.0)])]

    def test_undo_never_removes_the_start_point(self):
        drawn, _ = run(draw.polyline, "0,0", "u", "u", "5,5", "")
        assert drawn == [Polyline([(0, 0, 0.0), (5, 5, 0.0)])]


def test_rectangle_is_a_closed_four_corner_polyline():
    drawn, _ = run(draw.rectangle, "0,0", "@4,3")
    assert drawn == [Polyline([(0, 0, 0.0), (4, 0, 0.0), (4, 3, 0.0), (0, 3, 0.0)], closed=True)]


class TestCircle:
    def test_by_typed_radius(self):
        drawn, _ = run(curves.circle, "5,5", "2.5")
        assert drawn == [Circle((5, 5), 2.5)]

    def test_rejects_zero_and_negative_radius_and_keeps_asking(self):
        _, session = run(curves.circle, "0,0")
        assert session.submit_text("0") == INVALID_ANSWER["number"]
        assert session.submit_text("-3") == INVALID_ANSWER["number"]
        assert session.active

    def test_radius_from_a_click_is_the_distance_to_the_center(self):
        committed, session = start(curves.circle)
        session.submit_point((10, 10))
        session.submit_point((13, 14))
        assert committed == [Circle((10, 10), 5.0)]

    def test_click_at_the_center_does_not_answer_the_radius_prompt(self):
        committed, session = start(curves.circle)
        session.submit_point((10, 10))
        session.submit_point((10, 10))
        assert session.active and committed == []

    def test_diameter_option_takes_a_typed_diameter(self):
        drawn, _ = run(curves.circle, "5,5", "d", "10")
        assert drawn == [Circle((5, 5), 5.0)]

    def test_diameter_click_makes_the_circle_pass_through_the_click(self):
        committed, session = start(curves.circle)
        session.submit_point((0, 0))
        session.submit_text("d")
        session.submit_point((3, 4))
        assert committed == [Circle((0, 0), 5.0)]

    def test_two_points_are_the_ends_of_a_diameter(self):
        drawn, _ = run(curves.circle, "2p", "0,0", "10,0")
        assert drawn == [Circle((5, 0), 5.0)]

    def test_two_identical_points_ask_again(self):
        drawn, session = run(curves.circle, "2p", "3,3", "3,3")
        assert drawn == [] and session.active

    def test_three_points_on_the_circle(self):
        drawn, _ = run(curves.circle, "3p", "10,0", "0,10", "-10,0")
        assert drawn[0].center == pytest.approx((0, 0), abs=1e-9)
        assert drawn[0].radius == pytest.approx(10)

    def test_three_points_in_a_line_ask_again(self):
        drawn, session = run(curves.circle, "3p", "0,0", "5,0", "10,0")
        assert drawn == [] and "in a line" in session.prompt.message


class TestArc:
    def test_through_three_points_runs_counter_clockwise_through_the_middle(self):
        drawn, _ = run(curves.arc, "10,0", "0,10", "-10,0")
        (arc,) = drawn
        assert isinstance(arc, Arc)
        assert arc.center == pytest.approx((0, 0), abs=1e-9)
        assert arc.radius == pytest.approx(10)
        assert (arc.start_angle, arc.end_angle) == pytest.approx((0, 180))

    def test_drawn_clockwise_swaps_start_and_end(self):
        drawn, _ = run(curves.arc, "-10,0", "0,10", "10,0")
        assert (drawn[0].start_angle, drawn[0].end_angle) == pytest.approx((0, 180))

    def test_with_collinear_points_asks_again(self):
        _, session = run(curves.arc, "0,0", "5,0", "10,0")
        assert session.active
        assert "in a line" in session.prompt.message

    def test_center_start_end(self):
        drawn, _ = run(curves.arc, "c", "0,0", "10,0", "0,10")
        assert drawn == [Arc((0, 0), 10.0, 0.0, 90.0)]

    def test_start_center_end(self):
        drawn, _ = run(curves.arc, "10,0", "c", "0,0", "0,10")
        assert drawn == [Arc((0, 0), 10.0, 0.0, 90.0)]

    def test_start_end_center(self):
        drawn, _ = run(curves.arc, "10,0", "e", "0,10", "0,0")
        assert drawn == [Arc((0, 0), 10.0, 0.0, 90.0)]

    def test_the_end_point_only_sets_the_direction_so_it_may_be_off_the_circle(self):
        drawn, _ = run(curves.arc, "c", "0,0", "10,0", "0,50")
        assert drawn == [Arc((0, 0), 10.0, 0.0, 90.0)]

    def test_center_start_end_with_the_end_in_the_start_direction_asks_again(self):
        drawn, session = run(curves.arc, "c", "0,0", "10,0", "20,0")
        assert drawn == [] and "does not make an arc" in session.prompt.message

    def test_start_point_on_the_center_asks_again(self):
        _, session = run(curves.arc, "c", "4,4", "4,4")
        assert "is the center" in session.prompt.message

    def test_center_on_the_start_point_asks_again(self):
        _, session = run(curves.arc, "4,4", "c", "4,4")
        assert "is the start point" in session.prompt.message


class TestText:
    def test_uses_default_height_and_rotation_on_enter(self):
        drawn, _ = run(draw.text, "1,2", "", "", "Hello")
        assert drawn == [Text((1, 2), "Hello", draw.DEFAULT_TEXT_HEIGHT, 0.0)]

    def test_with_height_and_rotation(self):
        drawn, _ = run(draw.text, "1,2", "4", "30", "Hi")
        assert drawn == [Text((1, 2), "Hi", 4.0, 30.0)]

    def test_rotation_may_be_negative_or_zero_but_not_a_word(self):
        drawn, session = run(draw.text, "0,0", "", "-45")
        assert session.submit_text("Hello") is None
        assert drawn == [Text((0, 0), "Hello", draw.DEFAULT_TEXT_HEIGHT, -45.0)]
        _, session = run(draw.text, "0,0", "")
        assert session.submit_text("sideways") == INVALID_ANSWER["angle"]

    def test_rotation_from_a_click_is_the_direction_from_the_start_point(self):
        committed, session = start(draw.text)
        session.submit_point((10, 10))
        session.submit_text("")
        session.submit_point((10, 20))
        session.submit_text("Up")
        assert committed == [Text((10, 10), "Up", draw.DEFAULT_TEXT_HEIGHT, 90.0)]

    def test_escape_draws_nothing(self):
        drawn, session = run(draw.text, "1,2")
        session.cancel()
        assert drawn == [] and not session.active


def test_escape_stops_any_command_without_drawing():
    for name in ("RECTANGLE", "CIRCLE", "ARC", "TEXT"):
        committed, session = start(DRAWING[name])
        session.submit_text("1,1")
        session.cancel()
        assert committed == [] and not session.active, name


def test_every_command_previews_something_while_the_cursor_moves():
    previews = {
        "LINE": ("0,0",),
        "PLINE": ("0,0",),
        "RECTANGLE": ("0,0",),
        "CIRCLE": ("0,0",),
        "ARC": ("0,0", "5,5"),
    }
    for name, answers in previews.items():
        _, session = run(DRAWING[name], *answers)
        assert session.prompt.preview((9, 3)), name


@pytest.mark.parametrize(
    "command, answers",
    [
        (curves.circle, ("2p", "0,0")),
        (curves.circle, ("3p", "0,0", "5,5")),
        (curves.arc, ("c", "0,0")),
        (curves.arc, ("c", "0,0", "5,0")),
        (curves.arc, ("5,0", "c")),
        (curves.arc, ("5,0", "e", "0,5")),
        (draw.text, ("0,0", "")),
    ],
)
def test_option_paths_also_preview_the_cursor(command, answers):
    _, session = run(command, *answers)
    assert session.prompt.preview is not None
    assert session.prompt.preview((7, 7))


def test_aliases_resolve_to_commands():
    assert resolve("l") == "LINE"
    assert resolve("pl") == "PLINE"
    assert resolve("rec") == "RECTANGLE"
    assert resolve("Circle") == "CIRCLE"
    assert resolve("nope") is None




