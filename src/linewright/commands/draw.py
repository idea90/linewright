from collections.abc import Callable

from linewright.commands.prompts import Command, Drawn, Prompt, Retract
from linewright.model.entities import Entity, Line, Polyline, Text
from linewright.model.geometry import Point

DEFAULT_TEXT_HEIGHT = 2.5
DEFAULT_TEXT_ROTATION = 0.0
CLOSE = "C"
UNDO = "U"
KEYWORD_NAMES = {CLOSE: "Close", UNDO: "Undo"}


def polyline_from(points: list[Point], closed: bool = False) -> Polyline:
    return Polyline([(x, y, 0.0) for x, y in points], closed)


def _rectangle(first: Point, other: Point) -> Polyline:
    corners = [first, (other[0], first[1]), other, (first[0], other[1])]
    return polyline_from(corners, closed=True)


def line_preview(start: Point) -> Callable[[Point], list[Entity]]:
    return lambda cursor: [Line(start, cursor)]


def _next_point_prompt(points: list[Point], preview: Callable[[Point], list[Entity]]) -> Prompt:
    """The prompt for the next point of a chain: Undo from the first segment, Close from the third point."""
    keywords = ([CLOSE] if len(points) >= 3 else []) + [UNDO]
    names = "/".join(KEYWORD_NAMES[keyword] for keyword in keywords)
    return Prompt("point", f"Specify next point or [{names}]:", points[-1], tuple(keywords), preview=preview)


def line() -> Command:
    points: list[Point] = []
    while True:
        if not points:
            first = yield Prompt("point", "Specify first point:")
            if first is None:
                return
            points.append(first)
            continue
        answer = yield _next_point_prompt(points, line_preview(points[-1]))
        if answer is None:
            return
        if answer == UNDO:
            if len(points) > 1:
                yield Retract()
            points.pop()
        elif answer == CLOSE:
            yield Drawn([Line(points[-1], points[0])])
            return
        else:
            yield Drawn([Line(points[-1], answer)])
            points.append(answer)


def polyline() -> Command:
    first = yield Prompt("point", "Specify start point:")
    if first is None:
        return
    points = [first]
    while True:
        answer = yield _next_point_prompt(points, lambda cursor: [polyline_from([*points, cursor])])
        if answer == CLOSE:
            yield Drawn([polyline_from(points, closed=True)])
            return
        if answer is None:
            if len(points) >= 2:
                yield Drawn([polyline_from(points)])
            return
        if answer == UNDO:
            if len(points) > 1:
                points.pop()
        else:
            points.append(answer)


def rectangle() -> Command:
    first = yield Prompt("point", "Specify first corner point:")
    if first is None:
        return
    other = yield Prompt(
        "point", "Specify other corner point:", first, preview=lambda cursor: [_rectangle(first, cursor)]
    )
    if other is not None:
        yield Drawn([_rectangle(first, other)])


def text() -> Command:
    position = yield Prompt("point", "Specify start point of text:")
    if position is None:
        return
    height = yield Prompt(
        "number", f"Specify height <{DEFAULT_TEXT_HEIGHT}>:", position, default=DEFAULT_TEXT_HEIGHT
    )
    rotation = yield Prompt(
        "angle",
        f"Specify rotation angle of text <{DEFAULT_TEXT_ROTATION:g}>:",
        position,
        default=DEFAULT_TEXT_ROTATION,
        preview=line_preview(position),
    )
    content = yield Prompt("text", "Enter text:")
    if content is not None:
        yield Drawn([Text(position, content, height, rotation)])

