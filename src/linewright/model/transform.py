import math
from dataclasses import dataclass, replace

from linewright.model.entities import Arc, Circle, Dimension, Entity, Line, Polyline, Text
from linewright.model.geometry import Point


@dataclass(frozen=True)
class Similarity:
    """A move, rotation, uniform scale or mirror: x' = a*x + b*y + tx, y' = c*x + d*y + ty."""

    a: float
    b: float
    c: float
    d: float
    tx: float = 0.0
    ty: float = 0.0

    def point(self, point: Point) -> Point:
        return (self.a * point[0] + self.b * point[1] + self.tx, self.c * point[0] + self.d * point[1] + self.ty)

    def angle(self, degrees: float) -> float:
        """Where a direction given in degrees points after the transform, in 0 to 360."""
        radians = math.radians(degrees)
        x = self.a * math.cos(radians) + self.b * math.sin(radians)
        y = self.c * math.cos(radians) + self.d * math.sin(radians)
        return math.degrees(math.atan2(y, x)) % 360.0

    @property
    def determinant(self) -> float:
        return self.a * self.d - self.b * self.c

    @property
    def scale(self) -> float:
        return math.sqrt(abs(self.determinant))

    @property
    def reflects(self) -> bool:
        return self.determinant < 0


def translation(dx: float, dy: float) -> Similarity:
    return Similarity(1, 0, 0, 1, dx, dy)


def _about(center: Point, a: float, b: float, c: float, d: float) -> Similarity:
    """The linear map with the given matrix that leaves the center point where it is."""
    x, y = center
    return Similarity(a, b, c, d, x - (a * x + b * y), y - (c * x + d * y))


def rotation(center: Point, degrees: float) -> Similarity:
    cos, sin = math.cos(math.radians(degrees)), math.sin(math.radians(degrees))
    return _about(center, cos, -sin, sin, cos)


def scaling(base: Point, factor: float) -> Similarity:
    return _about(base, factor, 0, 0, factor)


def reflection(first: Point, second: Point) -> Similarity:
    """Mirror across the line through two different points."""
    twice = 2 * math.atan2(second[1] - first[1], second[0] - first[0])
    return _about(first, math.cos(twice), math.sin(twice), math.sin(twice), -math.cos(twice))


def transform_entity(entity: Entity, sim: Similarity) -> Entity:
    """A transformed copy. Text keeps its direction when mirrored, so it stays readable."""
    if isinstance(entity, Line):
        return replace(entity, start=sim.point(entity.start), end=sim.point(entity.end))
    if isinstance(entity, Circle):
        return replace(entity, center=sim.point(entity.center), radius=entity.radius * sim.scale)
    if isinstance(entity, Arc):
        first, last = sim.angle(entity.start_angle), sim.angle(entity.end_angle)
        if sim.reflects:
            first, last = last, first
        return replace(
            entity, center=sim.point(entity.center), radius=entity.radius * sim.scale, start_angle=first, end_angle=last
        )
    if isinstance(entity, Polyline):
        flip = -1.0 if sim.reflects else 1.0
        vertices = [(*sim.point((x, y)), bulge * flip) for x, y, bulge in entity.vertices]
        return replace(entity, vertices=vertices)
    if isinstance(entity, Text):
        rotation_degrees = entity.rotation if sim.reflects else sim.angle(entity.rotation)
        return replace(
            entity,
            position=sim.point(entity.position),
            height=entity.height * sim.scale,
            rotation=rotation_degrees,
        )
    if isinstance(entity, Dimension):
        return replace(
            entity,
            start=sim.point(entity.start),
            end=sim.point(entity.end),
            line_point=sim.point(entity.line_point),
            angle=sim.angle(entity.angle),
        )
    raise TypeError(f"cannot transform {type(entity).__name__}")
