from dataclasses import dataclass

from linewright.model.entities import Entity
from linewright.model.geometry import Point
from linewright.snap.osnap import find_osnap
from linewright.snap.tracking import constrain_ortho, constrain_polar, snap_to_grid

DEFAULT_POLAR_INCREMENT = 45.0


@dataclass
class SnapSettings:
    """The drafting aids on the status bar. show_grid only draws the grid; grid is snapping to it."""

    show_grid: bool = True
    osnap: bool = True
    ortho: bool = False
    polar: bool = False
    grid: bool = False
    polar_increment: float = DEFAULT_POLAR_INCREMENT

    def set(self, name: str, on: bool) -> None:
        """Turn a mode on or off. Ortho and polar tracking exclude each other."""
        setattr(self, name, on)
        if on and name == "ortho":
            self.polar = False
        if on and name == "polar":
            self.ortho = False


@dataclass(frozen=True)
class SnapScene:
    """What the cursor can snap to, in world units."""

    aperture: float
    grid_step: float
    entities: list[Entity]


@dataclass(frozen=True)
class SnapResult:
    point: Point
    kind: str | None  # the object snap that was used, if any


def resolve_point(cursor: Point, base: Point | None, settings: SnapSettings, scene: SnapScene) -> SnapResult:
    """Object snaps win. Otherwise the grid rounds the cursor, then ortho or polar constrains it to the base."""
    if settings.osnap:
        hit = find_osnap(cursor, scene.aperture, scene.entities, base)
        if hit is not None:
            return SnapResult(hit.point, hit.kind)
    point = snap_to_grid(cursor, scene.grid_step) if settings.grid else cursor
    if base is not None and settings.ortho:
        point = constrain_ortho(base, point)
    elif base is not None and settings.polar:
        point = constrain_polar(base, point, settings.polar_increment)
    return SnapResult(point, None)

