from copy import copy
from dataclasses import dataclass, replace

from PySide6.QtGui import QUndoCommand

from linewright.canvas.sync import SceneSync
from linewright.model.document import Document, Layer
from linewright.model.entities import Entity, copy_state


class AddEntities(QUndoCommand):
    def __init__(self, document: Document, sync: SceneSync, entities: list[Entity]) -> None:
        super().__init__(f"Draw {type(entities[0]).__name__}")
        self._document = document
        self._sync = sync
        self._entities = entities

    def redo(self) -> None:
        for entity in self._entities:
            self._document.add(entity)
            self._sync.add(entity)

    def undo(self) -> None:
        self._document.remove_all(self._entities)
        for entity in self._entities:
            self._sync.remove(entity)


class RemoveEntities(QUndoCommand):
    def __init__(self, document: Document, sync: SceneSync, entities: list[Entity]) -> None:
        super().__init__("Erase")
        self._document = document
        self._sync = sync
        self._entities = entities
        self._positions: list[int] = []

    def redo(self) -> None:
        self._positions = self._document.positions_of(self._entities)
        self._document.remove_all(self._entities)
        for entity in self._entities:
            self._sync.remove(entity)

    def undo(self) -> None:
        # Put them back where they were, lowest position first so the later ones land correctly.
        for position, entity in sorted(zip(self._positions, self._entities), key=lambda pair: pair[0]):
            self._document.insert(position, entity)
            self._sync.add(entity)


class ModifyEntities(QUndoCommand):
    """Changes entities in place, so everything that refers to them (selection, drawing order) stays valid."""

    def __init__(self, sync: SceneSync, changes: list[tuple[Entity, Entity]]) -> None:
        super().__init__("Modify")
        self._sync = sync
        self._targets = [target for target, _ in changes]
        self._before = [copy(target) for target in self._targets]
        self._after = [after for _, after in changes]

    def redo(self) -> None:
        self._apply(self._after)

    def undo(self) -> None:
        self._apply(self._before)

    def _apply(self, states: list[Entity]) -> None:
        for target, state in zip(self._targets, states):
            copy_state(target, state)
            self._sync.refresh(target)



@dataclass
class LayerState:
    """Everything a layer change can touch: the layers, which one is current, and objects renamed with a layer."""

    layers: dict[str, Layer]
    current: str
    entity_layers: list[tuple[Entity, str]]


class ChangeLayers(QUndoCommand):
    def __init__(
        self, document: Document, sync: SceneSync, before: LayerState, after: LayerState, text: str
    ) -> None:
        super().__init__(text)
        self._document = document
        self._sync = sync
        self._before = before
        self._after = after

    def redo(self) -> None:
        self._apply(self._after)

    def undo(self) -> None:
        self._apply(self._before)

    def _apply(self, state: LayerState) -> None:
        self._document.layers = {name: replace(layer) for name, layer in state.layers.items()}
        self._document.current_layer = state.current
        for entity, name in state.entity_layers:
            entity.layer = name
        self._sync.refresh_all()
