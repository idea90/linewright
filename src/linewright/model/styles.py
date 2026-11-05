BYLAYER_LINETYPE = "ByLayer"
BYLAYER_COLOR = 256
LINETYPES = ("Continuous", "Dashed", "Center", "Hidden", "DashDot", "Phantom")

# Dash patterns for drawing a linetype on screen, in pixels: dash, gap, dash, gap...
DASH_PATTERNS = {
    "Dashed": (8.0, 5.0),
    "Center": (16.0, 4.0, 4.0, 4.0),
    "Hidden": (4.0, 4.0),
    "DashDot": (10.0, 4.0, 2.0, 4.0),
    "Phantom": (16.0, 4.0, 4.0, 4.0, 4.0, 4.0),
}

# AutoCAD color index, name. The first nine are the standard colors.
COLOR_CHOICES = (
    (1, "Red"),
    (2, "Yellow"),
    (3, "Green"),
    (4, "Cyan"),
    (5, "Blue"),
    (6, "Magenta"),
    (7, "White"),
    (8, "Gray"),
    (9, "Light Gray"),
)


def canonical_linetype(name: str) -> str:
    """The spelling Linewright uses for a linetype name from a file, which may be in any case. Unknown names stay as-is."""
    for known in (BYLAYER_LINETYPE, *LINETYPES):
        if known.lower() == name.lower():
            return known
    return name
