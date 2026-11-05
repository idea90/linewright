import math
from collections.abc import Callable, Generator

from linewright.commands.defaults import Defaults
from linewright.commands.draw import line_preview
from linewright.commands.prompts import Answer, Command, Drawn, Prompt
from linewright.model.entities import Dimension, Entity
from linewright.model.geometry import Point

SAME_POINT = "The points are the same."

AngleChooser = Callable[[Point, Point, Point], float]


def _linear_angle(start: Point, end: Point, cursor: Point) -> float:
    """Horizontal when the cursor sits above or below the two points more than beside them, otherwise vertical."""
    beside = max(min(start[0], end[0]) - cursor[0], cursor[0] - max(start[0], end[0]), 0.0)
    above = max(min(start[1], end[1]) - cursor[1], cursor[1] - max(start[1], end[1]), 0.0)
    return 0.0 if above >= beside else 90.0


def _aligned_angle(start: Point, end: Point, cursor: Point) -> float:
    return math.degrees(math.atan2(end[1] - start[1], end[0] - start[0]))


def _second_origin(start: Point) -> Generator[Prompt, Answer, Point | None]:
    message = "Specify second extension line origin:"
    while True:
        end = yield Prompt("point", message, start, preview=line_preview(start))
        if end is None or end != start:
            return end
        message = f"{SAME_POINT} Specify second extension line origin:"


def _dimension(choose_angle: AngleChooser, scale: float) -> Command:
    start = yield Prompt("point", "Specify first extension line origin:")
    if start is None:
        return
    end = yield from _second_origin(start)
    if end is None:
        return

    def dimension_at(cursor: Point) -> Dimension:
        return Dimension(start, end, cursor, choose_angle(start, end, cursor), scale)

    location = yield Prompt(
        "point", "Specify dimension line location:", end, preview=lambda cursor: [dimension_at(cursor)]
    )
    if location is not None:
        yield Drawn([dimension_at(location)])


def linear(selected: list[Entity], defaults: Defaults) -> Command:
    yield from _dimension(_linear_angle, defaults.dimension_scale)


def aligned(selected: list[Entity], defaults: Defaults) -> Command:
    yield from _dimension(_aligned_angle, defaults.dimension_scale)


def set_scale(selected: list[Entity], defaults: Defaults) -> Command:
    current = defaults.dimension_scale
    scale = yield Prompt("number", f"Enter new value for DIMSCALE <{current:g}>:", default=current)
    if scale is not None:
        defaults.dimension_scale = scale
