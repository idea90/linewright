from dataclasses import dataclass, fields

from linewright.model.geometry import Point
from linewright.model.styles import BYLAYER_COLOR, BYLAYER_LINETYPE

BYLAYER = BYLAYER_COLOR


@dataclass(kw_only=True)
class Entity:
    layer: str = "0"
    color: int = BYLAYER  # AutoCAD color index, 256 = by layer
    linetype: str = BYLAYER_LINETYPE


@dataclass
class Line(Entity):
    start: Point
    end: Point


@dataclass
class Circle(Entity):
    center: Point
    radius: float


@dataclass
class Arc(Entity):
    center: Point
    radius: float
    start_angle: float  # degrees, counter-clockwise
    end_angle: float


@dataclass
class Polyline(Entity):
    vertices: list[tuple[float, float, float]]  # x, y, bulge of the segment that starts here
    closed: bool = False


@dataclass
class Text(Entity):
    position: Point
    text: str
    height: float
    rotation: float = 0.0  # degrees


@dataclass
class Dimension(Entity):
    """A linear dimension: the distance between two points measured along the direction `angle`."""

    start: Point  # where the first extension line starts
    end: Point
    line_point: Point  # any point on the dimension line
    angle: float = 0.0  # degrees, direction of the dimension line
    scale: float = 1.0  # sizes text, arrows and gaps, like AutoCAD's DIMSCALE


def copy_state(target: Entity, source: Entity) -> None:
    """Give target the field values of source, keeping target's identity. Both must be the same type."""
    for field in fields(target):
        setattr(target, field.name, getattr(source, field.name))


