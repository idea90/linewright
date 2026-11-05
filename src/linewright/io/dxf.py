import math
from collections.abc import Callable

import ezdxf
from ezdxf.document import Drawing
from ezdxf.entities import DXFEntity
from ezdxf.entities.dimstyleoverride import DimStyleOverride
from ezdxf.math import Vec3

from linewright.io.layers import LinetypeNames, layer_name_as_defined, read_layers, write_layers
from linewright.model.dimension import (
    ARROW_LENGTH,
    EXTENSION_GAP,
    EXTENSION_OVERSHOOT,
    LENGTH_DECIMALS,
    TEXT_GAP,
    TEXT_HEIGHT,
)
from linewright.model.document import DEFAULT_LAYER_NAME, Document, Layer
from linewright.model.entities import BYLAYER, Arc, Circle, Dimension, Entity, Line, Polyline, Text
from linewright.model.geometry import Point
from linewright.model.styles import BYLAYER_LINETYPE, canonical_linetype

DEFAULT_DXF_VERSION = "R2018"
Z_AXIS = Vec3(0, 0, 1)
DIMENSION_STYLE = "Standard"
DIMENSION_TYPE_MASK = 7
LINEAR_DIMENSION, ALIGNED_DIMENSION = 0, 1
INSUNITS_MILLIMETERS = 4
# Millimeters in one drawing unit for each $INSUNITS code Linewright knows. Unitless (0) drawings are read as millimeters.
MM_PER_UNIT = {0: 1.0, 1: 25.4, 2: 304.8, 4: 1.0, 5: 10.0, 6: 1000.0}
# Full and short name of a drawing unit, by how many millimeters it is.
UNIT_NAMES = {
    1.0: ("millimeters", "mm"),
    10.0: ("centimeters", "cm"),
    1000.0: ("meters", "m"),
    25.4: ("inches", "in"),
    304.8: ("feet", "ft"),
}


def _xy(vector) -> Point:
    return (vector[0], vector[1])


def _style(e: DXFEntity) -> dict:
    linetype = canonical_linetype(e.dxf.get("linetype", BYLAYER_LINETYPE))
    if linetype.lower() == "byblock":
        linetype = BYLAYER_LINETYPE
    return {"layer": e.dxf.get("layer", DEFAULT_LAYER_NAME), "color": e.dxf.get("color", BYLAYER), "linetype": linetype}


def _line(e: DXFEntity) -> Entity:
    return Line(_xy(e.dxf.start), _xy(e.dxf.end), **_style(e))


def _circle(e: DXFEntity) -> Entity:
    return Circle(_xy(e.dxf.center), e.dxf.radius, **_style(e))


def _arc(e: DXFEntity) -> Entity:
    return Arc(_xy(e.dxf.center), e.dxf.radius, e.dxf.start_angle, e.dxf.end_angle, **_style(e))


def _lwpolyline(e: DXFEntity) -> Entity:
    return Polyline([tuple(p) for p in e.get_points("xyb")], e.closed, **_style(e))


def _polyline(e: DXFEntity) -> Entity | None:
    if not e.is_2d_polyline:
        return None
    vertices = [(*_xy(v.dxf.location), v.dxf.get("bulge", 0.0)) for v in e.vertices]
    return Polyline(vertices, e.is_closed, **_style(e))


def _text(e: DXFEntity) -> Entity:
    return Text(_xy(e.dxf.insert), e.dxf.text, e.dxf.height, e.dxf.get("rotation", 0.0), **_style(e))


def _mtext(e: DXFEntity) -> Entity:
    return Text(
        _xy(e.dxf.insert), e.plain_text(), e.dxf.char_height, e.dxf.get("rotation", 0.0), **_style(e)
    )


def _dimension(e: DXFEntity) -> Entity | None:
    kind = e.dxf.dimtype & DIMENSION_TYPE_MASK
    if kind not in (LINEAR_DIMENSION, ALIGNED_DIMENSION):
        return None
    start, end = _xy(e.dxf.defpoint2), _xy(e.dxf.defpoint3)
    if kind == ALIGNED_DIMENSION:
        angle = math.degrees(math.atan2(end[1] - start[1], end[0] - start[0]))
    else:
        angle = e.dxf.get("angle", 0.0)
    scale = DimStyleOverride(e).get("dimscale", 1.0)
    return Dimension(start, end, _xy(e.dxf.defpoint), angle, scale, **_style(e))


_READERS: dict[str, Callable[[DXFEntity], Entity | None]] = {
    "LINE": _line,
    "CIRCLE": _circle,
    "ARC": _arc,
    "LWPOLYLINE": _lwpolyline,
    "POLYLINE": _polyline,
    "TEXT": _text,
    "MTEXT": _mtext,
    "DIMENSION": _dimension,
}


def _in_world_plane(e: DXFEntity) -> bool:
    return not e.dxf.hasattr("extrusion") or Vec3(e.dxf.extrusion).isclose(Z_AXIS)


def document_from_drawing(drawing: Drawing) -> Document:
    document = Document(source=drawing)
    document.layers = read_layers(drawing)
    for e in drawing.modelspace():
        read = _READERS.get(e.dxftype())
        entity = read(e) if read and _in_world_plane(e) else None
        if entity is None:
            document.skipped[e.dxftype()] += 1
            continue
        entity.layer = _defined_layer(document, entity.layer)
        document.entities.append(entity)
        document.loaded_handles.add(e.dxf.handle)
    return document


def _defined_layer(document: Document, name: str) -> str:
    """The layer name as the table spells it. A layer used but never defined is added."""
    defined = layer_name_as_defined(document.layers, name)
    if defined not in document.layers:
        document.layers[defined] = Layer(defined)
    return defined


def _add_line(msp, e: Line, attribs: dict):
    return msp.add_line(e.start, e.end, dxfattribs=attribs)


def _add_circle(msp, e: Circle, attribs: dict):
    return msp.add_circle(e.center, e.radius, dxfattribs=attribs)


def _add_arc(msp, e: Arc, attribs: dict):
    return msp.add_arc(e.center, e.radius, e.start_angle, e.end_angle, dxfattribs=attribs)


def _add_polyline(msp, e: Polyline, attribs: dict):
    return msp.add_lwpolyline(e.vertices, format="xyb", close=e.closed, dxfattribs=attribs)


def _add_text(msp, e: Text, attribs: dict):
    text = msp.add_text(e.text, height=e.height, rotation=e.rotation, dxfattribs=attribs)
    return text.set_placement(e.position)


def _add_dimension(msp, e: Dimension, attribs: dict):
    override = {"dimtxt": TEXT_HEIGHT, "dimasz": ARROW_LENGTH, "dimexo": EXTENSION_GAP, "dimexe": EXTENSION_OVERSHOOT,
                "dimgap": TEXT_GAP, "dimdec": LENGTH_DECIMALS, "dimtad": 1, "dimscale": e.scale}
    style = msp.add_linear_dim(
        e.line_point, e.start, e.end, angle=e.angle, dimstyle=DIMENSION_STYLE, override=override, dxfattribs=attribs
    )
    style.render()
    return style.dimension


_WRITERS: dict[type, Callable] = {
    Line: _add_line,
    Circle: _add_circle,
    Arc: _add_arc,
    Polyline: _add_polyline,
    Text: _add_text,
    Dimension: _add_dimension,
}


def mm_per_unit(document: Document) -> float:
    """How many millimeters one drawing unit is. A drawing with no units set is read as millimeters."""
    if document.source is None:
        return 1.0
    return MM_PER_UNIT.get(document.source.header.get("$INSUNITS", 0), 1.0)


def unit_names(document: Document) -> tuple[str, str]:
    return UNIT_NAMES.get(mm_per_unit(document), UNIT_NAMES[1.0])


def _new_drawing() -> Drawing:
    drawing = ezdxf.new(DEFAULT_DXF_VERSION, setup=True)
    drawing.header["$INSUNITS"] = INSUNITS_MILLIMETERS
    return drawing


def _delete_stale(drawing: Drawing, stale: DXFEntity) -> None:
    """Remove an entity Linewright is about to write again. A dimension also leaves its drawn block behind."""
    block_name = stale.dxf.get("geometry") if stale.dxftype() == "DIMENSION" else None
    drawing.modelspace().delete_entity(stale)
    if block_name and block_name in drawing.blocks:
        drawing.blocks.delete_block(block_name, safe=False)


def drawing_from_document(document: Document) -> Drawing:
    """Write the document into its source drawing (or a new one), replacing the entities it loaded."""
    drawing = document.source or _new_drawing()
    modelspace = drawing.modelspace()
    for handle in document.loaded_handles:
        stale = drawing.entitydb.get(handle)
        if stale is not None:
            _delete_stale(drawing, stale)

    linetypes = LinetypeNames(drawing)
    write_layers(document, drawing, linetypes)
    document.loaded_handles = set()
    for entity in document.entities:
        attribs = {"layer": entity.layer, "color": entity.color}
        if entity.linetype != BYLAYER_LINETYPE:
            attribs["linetype"] = linetypes.name(entity.linetype)
        written = _WRITERS[type(entity)](modelspace, entity, attribs)
        document.loaded_handles.add(written.dxf.handle)
    document.source = drawing
    return drawing





