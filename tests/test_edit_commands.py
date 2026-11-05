import pytest

from linewright.commands import edit
from linewright.commands.defaults import Defaults
from linewright.commands.session import INVALID_ANSWER, SELECT_HINT
from linewright.model.entities import Circle, Line, Text
from helpers import new_session


def run(command, selected, *answers):
    host, session = new_session(command, selected, Defaults())
    for answer in answers:
        assert session.active, f"command ended early before {answer!r}"
        error = session.submit_text(answer)
        assert error is None, error
    return host, session


def new_value(host, index=0):
    return host.modified[index][1]


class TestErase:
    def test_uses_the_selection_it_was_given(self):
        line = Line((0, 0), (1, 1))
        host, session = new_session(edit.erase, [line], Defaults())
        assert host.removed == [line]
        assert not session.active

    def test_asks_for_objects_when_nothing_is_selected(self):
        a, b = Line((0, 0), (1, 1)), Circle((0, 0), 1)
        host, session = new_session(edit.erase, [], Defaults())
        assert session.prompt.kind == "select"
        session.submit_objects([a, b])
        assert host.removed == [a, b]
        assert not session.active

    def test_enter_or_escape_with_nothing_chosen_erases_nothing(self):
        host, session = run(edit.erase, [], "")
        assert host.removed == [] and not session.active
        host, session = run(edit.erase, [])
        session.cancel()
        assert host.removed == [] and not session.active

    def test_typing_text_at_a_select_prompt_explains_what_to_do(self):
        _, session = run(edit.erase, [])
        assert session.submit_text("everything") == SELECT_HINT


class TestMove:
    def test_moves_by_the_vector_between_the_two_points(self):
        line = Line((0, 0), (10, 0))
        host, session = run(edit.move, [line], "0,0", "@5,3")
        assert host.modified == [(line, Line((5, 3), (15, 3)))]
        assert not session.active

    def test_the_original_is_not_touched_by_the_command_itself(self):
        line = Line((0, 0), (10, 0))
        run(edit.move, [line], "1,1", "4,5")
        assert line == Line((0, 0), (10, 0))

    def test_asks_for_objects_first_when_nothing_is_selected(self):
        line = Line((0, 0), (10, 0))
        host, session = new_session(edit.move, [], Defaults())
        session.submit_objects([line])
        assert session.prompt.message == "Specify base point:"
        session.submit_text("0,0")
        session.submit_text("2,0")
        assert host.modified == [(line, Line((2, 0), (12, 0)))]

    def test_escape_changes_nothing(self):
        line = Line((0, 0), (10, 0))
        host, session = run(edit.move, [line], "0,0")
        session.cancel()
        assert host.modified == [] and not session.active

    def test_preview_shows_the_moved_copies_following_the_cursor(self):
        _, session = run(edit.move, [Line((0, 0), (10, 0))], "0,0")
        (shown,) = session.prompt.preview((3, 4))
        assert shown == Line((3, 4), (13, 4))

    def test_preview_is_capped_so_big_selections_do_not_make_the_cursor_lag(self):
        many = [Line((i, 0), (i, 1)) for i in range(edit.MAX_PREVIEW_ENTITIES * 3)]
        _, session = run(edit.move, many, "0,0")
        assert len(session.prompt.preview((1, 1))) == edit.MAX_PREVIEW_ENTITIES


class TestCopy:
    def test_each_second_point_makes_another_copy_until_enter(self):
        line = Line((0, 0), (10, 0))
        host, session = run(edit.copy, [line], "0,0", "0,5", "0,10", "")
        assert host.added == [Line((0, 5), (10, 5)), Line((0, 10), (10, 10))]
        assert host.modified == []
        assert not session.active

    def test_undo_takes_back_the_last_copy(self):
        host, _ = run(edit.copy, [Line((0, 0), (10, 0))], "0,0", "0,5", "0,10", "u")
        assert host.added == [Line((0, 5), (10, 5))]

    def test_undo_with_no_copies_does_nothing(self):
        host, session = run(edit.copy, [Line((0, 0), (10, 0))], "0,0", "u")
        assert host.added == [] and session.active

    def test_copies_are_independent_entities(self):
        line = Line((0, 0), (10, 0))
        host, _ = run(edit.copy, [line], "0,0", "0,0")
        assert host.added[0] is not line


class TestRotate:
    def test_typed_angle(self):
        line = Line((1, 0), (2, 0))
        host, _ = run(edit.rotate, [line], "0,0", "90")
        assert new_value(host).start == pytest.approx((0, 1), abs=1e-9)
        assert new_value(host).end == pytest.approx((0, 2), abs=1e-9)

    def test_negative_angle_turns_clockwise(self):
        host, _ = run(edit.rotate, [Line((1, 0), (2, 0))], "0,0", "-90")
        assert new_value(host).start == pytest.approx((0, -1), abs=1e-9)

    def test_angle_from_a_click_is_the_direction_from_the_base_point(self):
        host, session = new_session(edit.rotate, [Line((1, 0), (2, 0))], Defaults())
        session.submit_text("0,0")
        session.submit_point((0, 5))
        assert new_value(host).start == pytest.approx((0, 1), abs=1e-9)

    def test_preview_follows_the_cursor_direction(self):
        _, session = run(edit.rotate, [Line((1, 0), (2, 0))], "0,0")
        (shown,) = session.prompt.preview((0, 9))
        assert shown.start == pytest.approx((0, 1), abs=1e-9)

    def test_a_word_is_not_an_angle(self):
        _, session = run(edit.rotate, [Line((1, 0), (2, 0))], "0,0")
        assert session.submit_text("round") == INVALID_ANSWER["angle"]


class TestScale:
    def test_typed_factor(self):
        host, _ = run(edit.scale, [Circle((10, 0), 2)], "0,0", "3")
        assert new_value(host) == Circle((30, 0), 6)

    def test_zero_and_negative_factors_are_refused(self):
        _, session = run(edit.scale, [Circle((10, 0), 2)], "0,0")
        assert session.submit_text("0") == INVALID_ANSWER["number"]
        assert session.submit_text("-2") == INVALID_ANSWER["number"]
        assert session.active

    def test_click_distance_from_the_base_is_the_factor(self):
        host, session = new_session(edit.scale, [Circle((10, 0), 2)], Defaults())
        session.submit_text("0,0")
        session.submit_point((0, 2))
        assert new_value(host) == Circle((20, 0), 4)

    def test_preview_at_the_base_point_shows_nothing(self):
        _, session = run(edit.scale, [Circle((10, 0), 2)], "0,0")
        assert session.prompt.preview((0, 0)) == []
        assert session.prompt.preview((0, 2)) == [Circle((20, 0), 4)]

    def test_text_grows_with_the_scale(self):
        host, _ = run(edit.scale, [Text((0, 0), "Hi", 2.0)], "0,0", "2")
        assert new_value(host).height == pytest.approx(4.0)


class TestMirror:
    def test_enter_keeps_the_originals_and_adds_mirrored_copies(self):
        line = Line((1, 1), (3, 2))
        host, session = run(edit.mirror, [line], "0,0", "10,0", "")
        assert host.added == [Line((1, -1), (3, -2))]
        assert host.modified == [] and host.removed == []
        assert not session.active

    def test_yes_replaces_the_originals(self):
        line = Line((1, 1), (3, 2))
        host, _ = run(edit.mirror, [line], "0,0", "10,0", "y")
        assert host.modified == [(line, Line((1, -1), (3, -2)))]
        assert host.added == []

    def test_escape_at_the_last_question_cancels_the_mirror(self):
        host, session = run(edit.mirror, [Line((1, 1), (3, 2))], "0,0", "10,0")
        session.cancel()
        assert host.added == [] and host.modified == [] and not session.active

    def test_the_mirror_line_needs_two_different_points(self):
        host, session = run(edit.mirror, [Line((1, 1), (3, 2))], "5,5", "5,5")
        assert session.active and "two different points" in session.prompt.message
        session.submit_text("5,9")
        session.submit_text("")
        assert host.added[0].start == pytest.approx((9, 1))
        assert host.added[0].end == pytest.approx((7, 2))

    def test_preview_mirrors_across_the_line_to_the_cursor(self):
        _, session = run(edit.mirror, [Line((1, 1), (3, 2))], "0,0")
        assert session.prompt.preview((10, 0)) == [Line((1, -1), (3, -2))]
        assert session.prompt.preview((0, 0)) == []


class TestOffset:
    def test_offsets_one_object_at_a_time_until_enter(self):
        line = Line((0, 0), (10, 0))
        host, session = run(edit.offset, [], "2")
        assert session.prompt.kind == "pick"
        session.submit_pick(line, (0, 0))
        session.submit_text("5,5")
        assert host.added == [Line((0, 2), (10, 2))]
        assert session.prompt.kind == "pick"
        session.submit_pick(line, (0, 0))
        session.submit_text("5,-5")
        assert host.added[-1] == Line((0, -2), (10, -2))
        session.submit_text("")
        assert not session.active

    def test_the_distance_must_be_greater_than_zero(self):
        _, session = new_session(edit.offset, [], Defaults())
        assert session.submit_text("0") == INVALID_ANSWER["number"]
        assert session.submit_text("-1") == INVALID_ANSWER["number"]

    def test_an_object_that_cannot_be_offset_is_reported_and_the_prompt_returns(self):
        host, session = run(edit.offset, [], "2")
        session.submit_pick(Text((0, 0), "x", 1.0), (0, 0))
        session.submit_text("1,1")
        assert host.added == []
        assert "cannot be offset" in session.prompt.message

    def test_preview_shows_the_offset_on_the_cursor_side(self):
        _, session = run(edit.offset, [], "2")
        session.submit_pick(Line((0, 0), (10, 0)), (0, 0))
        assert session.prompt.preview((5, 9)) == [Line((0, 2), (10, 2))]
        assert session.prompt.preview((5, -9)) == [Line((0, -2), (10, -2))]

    def test_preview_is_empty_when_the_object_cannot_be_offset(self):
        _, session = run(edit.offset, [], "5")
        session.submit_pick(Circle((0, 0), 3), (0, 0))
        assert session.prompt.preview((0, 0)) == []

    def test_clicks_do_not_answer_the_pick_or_distance_prompts_as_points(self):
        host, session = new_session(edit.offset, [], Defaults())
        session.submit_point((1, 1))
        assert session.prompt.kind == "number"
        session.submit_text("2")
        session.submit_point((1, 1))
        assert session.prompt.kind == "pick"
        assert host.added == []


