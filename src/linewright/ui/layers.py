from dataclasses import replace

from PySide6.QtGui import QUndoStack

from linewright.canvas.sync import SceneSync
from linewright.model.document import DEFAULT_LAYER_NAME, Document, Layer
from linewright.model.entities import Entity
from linewright.ui.undo import ChangeLayers, LayerState


class LayerService:
    """Creating, renaming and changing layers. Each method returns a message for the user if it refused."""

    def __init__(self, sync: SceneSync, undo_stack: QUndoStack) -> None:
        self._sync = sync
        self._undo_stack = undo_stack
        self._document = Document()

    def set_document(self, document: Document) -> None:
        self._document = document

    def create(self) -> str:
        number = 1
        while any(name.lower() == f"layer{number}" for name in self._document.layers):
            number += 1
        name = f"Layer{number}"
        self._push("New layer", {**self._copy_layers(), name: Layer(name)})
        return name

    def rename(self, old: str, new: str) -> str | None:
        new = new.strip()
        if old == DEFAULT_LAYER_NAME:
            return "Layer 0 cannot be renamed."
        if not new:
            return "A layer needs a name."
        if new == old:
            return None
        if any(name.lower() == new.lower() and name != old for name in self._document.layers):
            return f"A layer called {new} already exists."
        layers = {(new if name == old else name): replace(layer, name=new if name == old else name)
                  for name, layer in self._document.layers.items()}
        affected = [entity for entity in self._document.entities if entity.layer == old]
        current = new if self._document.current_layer == old else self._document.current_layer
        self._push("Rename layer", layers, current, renamed=(affected, old, new))
        return None

    def delete(self, name: str) -> str | None:
        if name == DEFAULT_LAYER_NAME:
            return "Layer 0 cannot be deleted."
        if name == self._document.current_layer:
            return "The current layer cannot be deleted."
        if self._document.layer_in_use(name):
            return f"Layer {name} has objects on it."
        layers = self._copy_layers()
        del layers[name]
        self._push("Delete layer", layers)
        return None

    def set_color(self, name: str, color: int) -> str | None:
        return self._change(name, "Layer color", color=color)

    def set_linetype(self, name: str, linetype: str) -> str | None:
        return self._change(name, "Layer linetype", linetype=linetype)

    def set_visible(self, name: str, visible: bool) -> str | None:
        if not visible and name == self._document.current_layer:
            return "The current layer cannot be turned off."
        return self._change(name, "Layer on or off", visible=visible)

    def set_locked(self, name: str, locked: bool) -> str | None:
        return self._change(name, "Layer lock", locked=locked)

    def set_current(self, name: str) -> str | None:
        layer = self._document.layers.get(name)
        if layer is None:
            return f"There is no layer called {name}."
        if not layer.visible:
            return "A layer that is turned off cannot be current."
        if name != self._document.current_layer:
            self._push("Current layer", self._copy_layers(), current=name)
        return None

    def _change(self, name: str, text: str, **fields) -> str | None:
        layer = self._document.layers.get(name)
        if layer is None:
            return f"There is no layer called {name}."
        changed = replace(layer, **fields)
        if changed != layer:
            self._push(text, {**self._copy_layers(), name: changed})
        return None

    def _copy_layers(self) -> dict[str, Layer]:
        return {name: replace(layer) for name, layer in self._document.layers.items()}

    def _push(
        self,
        text: str,
        layers: dict[str, Layer],
        current: str | None = None,
        renamed: tuple[list[Entity], str, str] | None = None,
    ) -> None:
        document = self._document
        affected, old, new = renamed or ([], "", "")
        before = LayerState(self._copy_layers(), document.current_layer, [(e, old) for e in affected])
        after = LayerState(layers, current or document.current_layer, [(e, new) for e in affected])
        self._undo_stack.push(ChangeLayers(document, self._sync, before, after, text))
