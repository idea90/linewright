import math
from dataclasses import dataclass

from linewright.model.entities import Dimension
from linewright.model.geometry import Point

# Drawing units. These match the metric defaults of the DXF dimension style Linewright writes.
TEXT_HEIGHT = 2.5
ARROW_LENGTH = 2.5
ARROW_HALF_WIDTH = ARROW_LENGTH / 6
EXTENSION_GAP = 0.625  # the extension line stops short of the measured point
EXTENSION_OVERSHOOT = 1.25  # and runs past the dimension line
TEXT_GAP = 0.625
LENGTH_DECIMALS = 2

Segment = tuple[Point, Point]


@dataclass(frozen=True)
class DimensionParts:
    """Everything a dimension draws, in drawing units."""

    extension_lines: list[Segment]
    dimension_line: Segment
    arrows: list[tuple[Point, Point, Point]]  # filled triangles, tip first
    text: str
    text_height: float
    text_anchor: Point  # middle of the text's baseline
    text_angle: float  # degrees, kept readable (never upside down)


def format_length(value: float) -> str:
    return f"{value:.{LENGTH_DECIMALS}f}"


def _direction(angle: float) -> Point:
    radians = math.radians(angle)
    return (math.cos(radians), math.sin(radians))


def _normal(direction: Point) -> Point:
    return (-direction[1], direction[0])


def _along(point: Point, vector: Point, distance: float) -> Point:
    return (point[0] + vector[0] * distance, point[1] + vector[1] * distance)


def _dot(first: Point, second: Point) -> float:
    return first[0] * second[0] + first[1] * second[1]


def _difference(first: Point, second: Point) -> Point:
    return (first[0] - second[0], first[1] - second[1])


def measurement(dimension: Dimension) -> float:
    return abs(_dot(_difference(dimension.end, dimension.start), _direction(dimension.angle)))


def _readable(angle: float) -> float:
    angle %= 360.0
    if 90.0 < angle <= 270.0:
        return angle - 180.0
    return angle - 360.0 if angle > 270.0 else angle


def _arrow(tip: Point, toward: Point, direction: Point, normal: Point, scale: float) -> tuple[Point, Point, Point]:
    """A triangle at the tip, pointing away from `toward`. A zero-length dimension falls back to the line direction."""
    run = _difference(toward, tip)
    length = math.hypot(*run)
    unit = (run[0] / length, run[1] / length) if length else direction
    base = _along(tip, unit, ARROW_LENGTH * scale)
    return (tip, _along(base, normal, ARROW_HALF_WIDTH * scale), _along(base, normal, -ARROW_HALF_WIDTH * scale))


def dimension_parts(dimension: Dimension) -> DimensionParts:
    direction = _direction(dimension.angle)
    normal = _normal(direction)
    origins = (dimension.start, dimension.end)
    offsets = [_dot(_difference(dimension.line_point, origin), normal) for origin in origins]
    ends = [_along(origin, normal, offset) for origin, offset in zip(origins, offsets)]
    extension_lines = []
    for origin, end, offset in zip(origins, ends, offsets):
        side = 1.0 if offset >= 0 else -1.0
        extension_lines.append(
            (
                _along(origin, normal, side * EXTENSION_GAP * dimension.scale),
                _along(end, normal, side * EXTENSION_OVERSHOOT * dimension.scale),
            )
        )

    text_angle = _readable(dimension.angle)
    above = _normal(_direction(text_angle))
    middle = ((ends[0][0] + ends[1][0]) / 2, (ends[0][1] + ends[1][1]) / 2)
    return DimensionParts(
        extension_lines=extension_lines,
        dimension_line=(ends[0], ends[1]),
        arrows=[
            _arrow(ends[0], ends[1], direction, normal, dimension.scale),
            _arrow(ends[1], ends[0], direction, normal, dimension.scale),
        ],
        text=format_length(measurement(dimension)),
        text_height=TEXT_HEIGHT * dimension.scale,
        text_anchor=_along(middle, above, TEXT_GAP * dimension.scale),
        text_angle=text_angle,
    )
