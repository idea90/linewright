import pytest

from linewright.commands import edit
from linewright.commands.defaults import Defaults
from linewright.commands.session import INVALID_ANSWER
from linewright.model.entities import Arc, Circle, Line, Text
from helpers import new_session

CUT_AT_5 = Line((5, -5), (5, 5))
CUT_AT_8 = Line((8, -5), (8, 5))


def start(command, selected, defaults=None):
    host, session = new_session(command, selected, defaults or Defaults())
    return host, session


def pick(session, entity, point, shift=False):
    session.submit_pick(entity, point, shift)


class TestTrim:
    def test_asks_for_cutting_edges_and_enter_means_all_of_them(self):
        _, session = start(edit.trim, [])
        assert session.prompt.kind == "select" and session.prompt.or_all
        assert "cutting edges" in session.prompt.message

    def test_trims_the_clicked_side_as_one_undo_step(self):
        line = Line((0, 0), (10, 0))
        host, session = start(edit.trim, [])
        session.submit_objects([CUT_AT_5, line])
        assert session.prompt.kind == "pick"
        pick(session, line, (8, 0))
        assert host.events == [("begin",), ("remove", [line]), ("add", [Line((0, 0), (5, 0))]), ("end",)]
        assert host.groups_closed == 1

    def test_selected_objects_are_the_cutting_edges_without_asking(self):
        line = Line((0, 0), (10, 0))
        host, session = start(edit.trim, [CUT_AT_5, line])
        assert session.prompt.kind == "pick"

    def test_nothing_to_cut_against_is_reported_and_the_prompt_returns(self):
        line = Line((0, 0), (10, 0))
        host, session = start(edit.trim, [CUT_AT_5, line])
        pick(session, line, (8, 0))
        far = Line((0, 20), (10, 20))
        pick(session, far, (8, 20))
        assert host.events.count(("begin",)) == 1
        assert "Nothing to trim against there." in session.prompt.message

    def test_a_trim_that_leaves_nothing_just_removes_the_object(self):
        stub = Line((4, 0), (6, 0))
        host, session = start(edit.trim, [Line((4.5, -1), (4.5, 1)), Line((5.5, -1), (5.5, 1)), stub])
        pick(session, stub, (5, 0))
        assert [e for e in host.events if e[0] == "remove"] == [("remove", [stub])]
        assert len(host.added) == 2

    def test_pieces_replace_the_object_as_a_cutting_edge(self):
        horizontal = Line((0, 0), (10, 0))
        vertical = Line((5, -5), (5, 5))
        host, session = start(edit.trim, [horizontal, vertical])
        pick(session, horizontal, (8, 0))
        (piece,) = host.added
        pick(session, vertical, (5, -4))
        assert host.added[-1] == Line((5, 0), (5, 5))  # still cut by the piece that replaced the horizontal line

    def test_shift_extends_instead(self):
        short = Line((0, 0), (5, 0))
        host, session = start(edit.trim, [Line((10, -5), (10, 5)), short])
        pick(session, short, (4, 0), shift=True)
        assert host.modified == [(short, Line((0, 0), (10, 0)))]

    def test_u_takes_back_the_last_trim_and_restores_the_cutting_edges(self):
        line = Line((0, 0), (10, 0))
        host, session = start(edit.trim, [CUT_AT_5, line])
        pick(session, line, (8, 0))
        session.submit_text("u")
        assert host.events[-1] == ("retract",)
        pick(session, line, (2, 0))
        assert host.added[-1] == Line((5, 0), (10, 0))

    def test_u_with_nothing_to_undo_does_nothing(self):
        host, session = start(edit.trim, [CUT_AT_5, Line((0, 0), (10, 0))])
        session.submit_text("u")
        assert host.events == [] and session.active

    def test_enter_and_escape_end_the_command(self):
        _, session = start(edit.trim, [CUT_AT_5, Line((0, 0), (10, 0))])
        session.submit_text("")
        assert not session.active

    def test_no_boundaries_chosen_ends_the_command(self):
        host, session = start(edit.trim, [])
        session.submit_text("")
        assert not session.active and host.events == []


class TestExtend:
    def test_extends_the_end_near_the_click_in_place(self):
        short = Line((0, 0), (5, 0))
        host, session = start(edit.extend, [Line((10, -5), (10, 5)), short])
        pick(session, short, (4, 0))
        assert host.modified == [(short, Line((0, 0), (10, 0)))]
        assert session.prompt.kind == "pick"

    def test_nothing_in_the_way(self):
        short = Line((0, 0), (5, 0))
        host, session = start(edit.extend, [Line((-10, -5), (-10, 5)), short])
        pick(session, short, (4, 0))
        assert host.modified == []
        assert "No boundary edge is in the way." in session.prompt.message

    def test_shift_trims_instead(self):
        line = Line((0, 0), (10, 0))
        host, session = start(edit.extend, [CUT_AT_5, line])
        pick(session, line, (8, 0), shift=True)
        assert host.added == [Line((0, 0), (5, 0))]

    def test_u_takes_back_the_last_extension(self):
        short = Line((0, 0), (5, 0))
        host, session = start(edit.extend, [Line((10, -5), (10, 5)), short])
        pick(session, short, (4, 0))
        session.submit_text("u")
        assert host.events[-1] == ("retract",)

    def test_prompt_wording(self):
        _, session = start(edit.extend, [CUT_AT_5, Line((0, 0), (10, 0))])
        assert session.prompt.message == edit.EXTEND_PROMPT
        _, session = start(edit.extend, [])
        assert "boundary edges" in session.prompt.message


class TestFillet:
    HORIZONTAL = Line((0, 0), (10, 0))
    VERTICAL = Line((0, 0), (0, 10))

    def test_prompt_shows_the_radius_in_use(self):
        _, session = start(edit.fillet, [], Defaults(fillet_radius=2.5))
        assert "radius 2.5" in session.prompt.message

    def test_radius_zero_joins_the_lines_at_the_corner(self):
        host, session = start(edit.fillet, [])
        pick(session, self.HORIZONTAL, (8, 0))
        pick(session, self.VERTICAL, (0, 8))
        assert not session.active
        assert host.modified == [(self.HORIZONTAL, Line((0, 0), (10, 0))), (self.VERTICAL, Line((0, 0), (0, 10)))]
        assert host.added == []

    def test_a_radius_adds_an_arc_and_everything_undoes_together(self):
        host, session = start(edit.fillet, [], Defaults(fillet_radius=2))
        pick(session, self.HORIZONTAL, (8, 0))
        pick(session, self.VERTICAL, (0, 8))
        assert host.groups_closed == 1
        (arc,) = host.added
        assert isinstance(arc, Arc)
        assert arc.center == pytest.approx((2, 2))
        assert [kind for kind, *_ in host.events] == ["begin", "modify", "add", "end"]

    def test_the_radius_option_sets_and_remembers_it(self):
        defaults = Defaults()
        _, session = start(edit.fillet, [], defaults)
        session.submit_text("r")
        assert session.prompt.kind == "length"
        session.submit_text("3")
        assert defaults.fillet_radius == 3
        assert "radius 3" in session.prompt.message

    def test_a_radius_of_zero_is_allowed_but_not_a_negative_one(self):
        defaults = Defaults(fillet_radius=4)
        _, session = start(edit.fillet, [], defaults)
        session.submit_text("r")
        assert session.submit_text("-1") == INVALID_ANSWER["length"]
        session.submit_text("0")
        assert defaults.fillet_radius == 0

    def test_enter_keeps_the_radius_that_was_there(self):
        defaults = Defaults(fillet_radius=4)
        _, session = start(edit.fillet, [], defaults)
        session.submit_text("r")
        session.submit_text("")
        assert defaults.fillet_radius == 4

    def test_a_click_does_not_answer_the_radius_prompt(self):
        _, session = start(edit.fillet, [])
        session.submit_text("r")
        session.submit_point((5, 5))
        assert session.prompt.kind == "length"

    @pytest.mark.parametrize(
        "first, second, radius, text",
        [
            (Line((0, 0), (10, 0)), Line((0, 3), (10, 3)), 1, "parallel"),
            (Line((0, 0), (3, 0)), Line((0, 0), (0, 10)), 5, "too short"),
            (Circle((0, 0), 5), Line((0, 0), (0, 10)), 1, "lines only"),
        ],
    )
    def test_problems_are_explained_and_the_command_carries_on(self, first, second, radius, text):
        host, session = start(edit.fillet, [], Defaults(fillet_radius=radius))
        pick(session, first, (2, 0))
        pick(session, second, (0, 8))
        assert text in session.prompt.message
        assert host.events == [] and session.active

    def test_the_same_line_twice(self):
        host, session = start(edit.fillet, [])
        pick(session, self.HORIZONTAL, (2, 0))
        pick(session, self.HORIZONTAL, (8, 0))
        assert "two different lines" in session.prompt.message

    def test_enter_ends_the_command(self):
        _, session = start(edit.fillet, [])
        session.submit_text("")
        assert not session.active

    def test_enter_at_the_second_prompt_ends_it_too(self):
        _, session = start(edit.fillet, [])
        pick(session, self.HORIZONTAL, (2, 0))
        session.submit_text("")
        assert not session.active

    def test_text_is_not_a_line(self):
        host, session = start(edit.fillet, [])
        pick(session, Text((0, 0), "x", 1.0), (0, 0))
        pick(session, self.VERTICAL, (0, 8))
        assert "lines only" in session.prompt.message

