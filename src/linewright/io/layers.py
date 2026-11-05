from ezdxf.document import Drawing
from ezdxf.tools.standards import linetypes as standard_linetypes

from linewright.model.document import DEFAULT_LAYER_NAME, DEFAULT_LINETYPE, Document, Layer
from linewright.model.styles import canonical_linetype

KEPT_LAYERS = (DEFAULT_LAYER_NAME.lower(), "defpoints")
# ezdxf's standard set has no Hidden linetype, so this one is defined here (name, description, pattern).
HIDDEN = ("HIDDEN", "Hidden __ __ __ __ __ __ __ __ __ __ __ __ __", [0.375, 0.25, -0.125])


def read_layers(drawing: Drawing) -> dict[str, Layer]:
    """The layers of a drawing. Frozen layers count as turned off."""
    layers = {}
    for entry in drawing.layers:
        name = entry.dxf.name
        linetype = canonical_linetype(entry.dxf.get("linetype", DEFAULT_LINETYPE))
        hidden = entry.is_off() or entry.is_frozen()
        layers[name] = Layer(name, entry.color, linetype, visible=not hidden, locked=entry.is_locked())
    return layers


def layer_name_as_defined(layers: dict[str, Layer], name: str) -> str:
    """DXF layer names ignore case, so an object may say "walls" for a layer defined as "WALLS"."""
    if name in layers:
        return name
    return next((defined for defined in layers if defined.lower() == name.lower()), name)


class LinetypeNames:
    """Finds, or adds from the standard set, the table entry for a linetype and gives its name in the file."""

    def __init__(self, drawing: Drawing) -> None:
        self._drawing = drawing
        known = [*standard_linetypes(), HIDDEN]
        self._standard = {name.lower(): (name, desc, pattern) for name, desc, pattern in known}

    def name(self, linetype: str) -> str:
        for entry in self._drawing.linetypes:
            if entry.dxf.name.lower() == linetype.lower():
                return entry.dxf.name
        if linetype.lower() in self._standard:
            name, description, pattern = self._standard[linetype.lower()]
            self._drawing.linetypes.add(name, pattern=pattern, description=description)
            return name
        return self.name(DEFAULT_LINETYPE)


def write_layers(document: Document, drawing: Drawing, linetypes: LinetypeNames) -> None:
    """Make the drawing's layer table match the document. Layers the user deleted go if nothing else uses them."""
    for layer in document.layers.values():
        entry = drawing.layers.get(layer.name) if drawing.layers.has_entry(layer.name) else None
        entry = entry or drawing.layers.add(layer.name)
        entry.color = layer.color
        entry.dxf.linetype = linetypes.name(layer.linetype)
        _set_visible(entry, layer.visible)
        if layer.locked:
            entry.lock()
        else:
            entry.unlock()
    _remove_deleted_layers(document, drawing)


def _set_visible(entry, visible: bool) -> None:
    if visible:
        entry.on()
        entry.thaw()
    elif not entry.is_frozen():
        entry.off()


def _remove_deleted_layers(document: Document, drawing: Drawing) -> None:
    defined = {name.lower() for name in document.layers}
    in_use = {e.dxf.layer.lower() for e in drawing.entitydb.values() if e.dxf.hasattr("layer")}
    for entry in list(drawing.layers):
        name = entry.dxf.name.lower()
        if name not in defined and name not in in_use and name not in KEPT_LAYERS:
            drawing.layers.remove(entry.dxf.name)





