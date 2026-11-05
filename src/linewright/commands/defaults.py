from dataclasses import dataclass


@dataclass
class Defaults:
    """Settings the drawing commands remember from one use to the next."""

    fillet_radius: float = 0.0
    dimension_scale: float = 1.0
