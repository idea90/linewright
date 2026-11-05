from collections.abc import Iterable

from PySide6.QtCore import QRectF
from PySide6.QtGui import QColor
from PySide6.QtWidgets import QGraphicsScene

from linewright import theme
from linewright.canvas.layer import EntityLayer, LayerItem, Look, Style
from linewright.model.document import Document
from linewright.model.entities import Entity

BASE_Z = 0
SELECTION_Z = 500
PREVIEW_Z = 1000
SELECTED_PEN_WIDTH = 2.0
PREVIEW_DASHES = (4.0, 4.0)  # in pixels
BULK_CHANGE = 200  # a selection change this large is cheaper to rebuild than to edit entity by entity


class SceneSync:
    """Keeps what the scene draws in step with the document: its entities, the selection and the rubber band."""

    def __init__(self, scene: QGraphicsScene) -> None:
        self._scene = scene  # keeps the scene, and with it the layer items, alive
        self._document = Document()
        self._base = self._add_layer(scene, BASE_Z)
        self._selection = self._add_layer(
            scene, SELECTION_Z, Look(color=QColor(theme.ACCENT), width=SELECTED_PEN_WIDTH, dashes=())
        )
        self._preview = self._add_layer(scene, PREVIEW_Z, Look(dashes=PREVIEW_DASHES), honor_layers=False)

    def _add_layer(
        self, scene: QGraphicsScene, z_value: float, look: Look = Look(), honor_layers: bool = True
    ) -> EntityLayer:
        item = LayerItem(lambda: self._document, look, honor_layers, z_value)
        scene.addItem(item)
        return item.layer

    def reload(self, document: Document) -> None:
        self._document = document
        self._base.reload(document.entities)
        self._selection.reload([], self._base.cell_size)
        self._preview.reload([], self._base.cell_size)

    def add(self, entity: Entity) -> None:
        self._base.add(entity)

    def remove(self, entity: Entity) -> None:
        self._base.remove(entity)
        if entity in self._selection:
            self._selection.remove(entity)

    def refresh(self, entity: Entity) -> None:
        """Redraw an entity whose values changed in place."""
        self._base.refresh(entity)
        if entity in self._selection:
            self._selection.refresh(entity)

    def refresh_all(self) -> None:
        """Redraw every entity, after layers changed what they look like."""
        self._base.refresh_all()
        self._selection.refresh_all()

    def entities_in(self, area: QRectF) -> list[Entity]:
        """The visible entities whose bounding box touches the area, in drawing order. Fast for any drawing size."""
        return self._base.entities_in(area)

    def extent(self) -> QRectF:
        return self._base.extent()

    def style_of(self, entity: Entity) -> Style:
        """How the entity is drawn right now, selection included."""
        layer = self._selection if entity in self._selection else self._base
        return layer.style_of(entity)

    def set_selected(self, entities: Iterable[Entity]) -> None:
        wanted = {id(entity): entity for entity in entities if entity in self._base}
        leaving = [entity for entity in self._selection.entities() if id(entity) not in wanted]
        arriving = [entity for entity in wanted.values() if entity not in self._selection]
        if len(leaving) + len(arriving) > BULK_CHANGE:
            self._selection.reload(wanted.values(), self._base.cell_size)
            return
        for entity in leaving:
            self._selection.remove(entity)
        for entity in arriving:
            self._selection.add(entity)

    def set_preview(self, entities: list[Entity]) -> None:
        self._preview.reload(entities, self._base.cell_size)

    def clear_preview(self) -> None:
        if len(self._preview):
            self._preview.clear()

    @property
    def selected(self) -> list[Entity]:
        return self._selection.entities()

    @property
    def previewing(self) -> list[Entity]:
        return self._preview.entities()
