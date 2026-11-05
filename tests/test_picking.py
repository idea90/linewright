from linewright.model.entities import Arc, Circle, Line, Polyline, Text
from linewright.selection.picking import entity_distance, pick_entity, select_in_box
from linewright.selection.selection import Selection


def test_pick_the_nearest_entity_within_the_aperture():
    near, far = Line((0, 0), (10, 0)), Line((0, 5), (10, 5))
    assert pick_entity([near, far], (5, 0.5), 1.0) is near
    assert pick_entity([near, far], (5, 4.8), 1.0) is far
    assert pick_entity([near, far], (5, 2.5), 1.0) is None


def test_pick_prefers_the_later_entity_when_they_overlap():
    below, above = Line((0, 0), (10, 0)), Line((0, 0), (10, 0))
    assert pick_entity([below, above], (5, 0), 1.0) is above


def test_pick_a_circle_by_its_outline_not_its_inside():
    circle = Circle((0, 0), 5)
    assert pick_entity([circle], (5.2, 0), 0.5) is circle
    assert pick_entity([circle], (0, 0), 0.5) is None


def test_pick_text_anywhere_inside_its_box():
    text = Text((0, 0), "Hello", 2.0)
    assert entity_distance(text, (3, 1)) == 0.0
    assert pick_entity([text], (3, 1), 0.1) is text
    assert pick_entity([text], (3, 5), 0.1) is None


def test_pick_follows_rotated_text():
    text = Text((0, 0), "Hello", 2.0, 90.0)
    assert pick_entity([text], (-1, 3), 0.1) is text
    assert pick_entity([text], (3, 1), 0.1) is None


class TestBox:
    def test_window_needs_the_whole_entity_inside(self):
        inside, straddling = Line((1, 1), (4, 1)), Line((1, 2), (20, 2))
        assert select_in_box([inside, straddling], (0, 0), (10, 5)) == [inside]

    def test_crossing_takes_anything_touched(self):
        inside, straddling, outside = Line((1, 1), (4, 1)), Line((1, 2), (20, 2)), Line((1, 9), (4, 9))
        assert select_in_box([inside, straddling, outside], (10, 5), (0, 0)) == [inside, straddling]

    def test_dragging_right_to_left_is_a_crossing_whatever_the_height(self):
        line = Line((0, 0), (20, 0))
        assert select_in_box([line], (10, -1), (5, 1)) == [line]
        assert select_in_box([line], (5, 1), (10, -1)) == []

    def test_a_line_passing_through_the_box_is_crossed_even_without_an_end_inside(self):
        line = Line((-5, 0), (15, 0))
        assert select_in_box([line], (10, 2), (0, -2)) == [line]

    def test_circle_window(self):
        circle = Circle((0, 0), 5)
        assert select_in_box([circle], (-6, -6), (6, 6)) == [circle]
        assert select_in_box([circle], (-4, -4), (4, 4)) == []

    def test_box_inside_a_circle_does_not_cross_it(self):
        assert select_in_box([Circle((0, 0), 5)], (3, 3), (-3, -3)) == []

    def test_crossing_a_circle_by_touching_its_outline(self):
        circle = Circle((0, 0), 5)
        assert select_in_box([circle], (6, 1), (3, -1)) == [circle]

    def test_arc_window_uses_the_arc_not_the_whole_circle(self):
        arc = Arc((0, 0), 5, 0, 90)
        assert select_in_box([arc], (-1, -1), (6, 6)) == [arc]
        assert select_in_box([arc], (-1, -1), (4, 4)) == []

    def test_arc_crossing_only_where_the_arc_exists(self):
        arc = Arc((0, 0), 5, 0, 90)
        assert select_in_box([arc], (-3, -1), (-6, 1)) == []
        assert select_in_box([arc], (6, 1), (4, -1)) == [arc]

    def test_polyline_window_and_crossing(self):
        shape = Polyline([(0, 0, 0.0), (4, 0, 0.0), (4, 3, 0.0)])
        assert select_in_box([shape], (-1, -1), (5, 4)) == [shape]
        assert select_in_box([shape], (-1, -1), (2, 4)) == []
        assert select_in_box([shape], (5, 4), (3, 2)) == [shape]

    def test_text_window_and_crossing(self):
        text = Text((0, 0), "Hi", 2.0)
        assert select_in_box([text], (-1, -1), (5, 5)) == [text]
        assert select_in_box([text], (-1, -1), (0.5, 5)) == []
        assert select_in_box([text], (0.5, 5), (-1, -1)) == [text]


class TestSelection:
    def test_membership_is_by_identity(self):
        first, twin = Line((0, 0), (1, 1)), Line((0, 0), (1, 1))
        selection = Selection()
        selection.add([first])
        assert first in selection
        assert twin not in selection

    def test_replace_add_remove_clear(self):
        a, b, c = Line((0, 0), (1, 0)), Line((0, 1), (1, 1)), Line((0, 2), (1, 2))
        selection = Selection()
        selection.replace([a, b])
        selection.add([b, c])
        assert len(selection) == 3
        selection.remove([a, a])
        assert list(selection) == [b, c]
        selection.clear()
        assert len(selection) == 0

    def test_keep_only_forgets_entities_that_left_the_drawing(self):
        a, b = Line((0, 0), (1, 0)), Line((0, 1), (1, 1))
        selection = Selection()
        selection.add([a, b])
        selection.keep_only([b])
        assert list(selection) == [b]

    def test_iterating_while_changing_is_safe(self):
        a, b = Line((0, 0), (1, 0)), Line((0, 1), (1, 1))
        selection = Selection()
        selection.add([a, b])
        for entity in selection:
            selection.remove([entity])
        assert len(selection) == 0

