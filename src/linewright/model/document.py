from collections import Counter
from dataclasses import dataclass, field
from pathlib import Path

from ezdxf.document import Drawing

from linewright.model.entities import BYLAYER, Entity
from linewright.model.styles import BYLAYER_LINETYPE

DEFAULT_LAYER_COLOR = 7
DEFAULT_LAYER_NAME = "0"
DEFAULT_LINETYPE = "Continuous"


@dataclass
class Layer:
    name: str
    color: int = DEFAULT_LAYER_COLOR
    linetype: str = DEFAULT_LINETYPE
    visible: bool = True
    locked: bool = False


def _default_layers() -> dict[str, Layer]:
    return {DEFAULT_LAYER_NAME: Layer(DEFAULT_LAYER_NAME)}


@dataclass
class Document:
    entities: list[Entity] = field(default_factory=list)
    layers: dict[str, Layer] = field(default_factory=_default_layers)
    current_layer: str = DEFAULT_LAYER_NAME
    current_color: int = BYLAYER  # what new objects get
    current_linetype: str = BYLAYER_LINETYPE
    path: Path | None = None
    # The loaded DXF drawing. Saving writes into it so content Linewright cannot edit yet is kept.
    source: Drawing | None = None
    loaded_handles: set[str] = field(default_factory=set)  # source entities this document replaces
    skipped: Counter[str] = field(default_factory=Counter)

    def add(self, entity: Entity) -> None:
        self.entities.append(entity)

    def insert(self, index: int, entity: Entity) -> None:
        self.entities.insert(index, entity)

    def positions_of(self, entities: list[Entity]) -> list[int]:
        """Where each entity sits, found in one pass however many there are. Equal-looking entities stay distinct."""
        positions = {id(entity): index for index, entity in enumerate(self.entities)}
        try:
            return [positions[id(entity)] for entity in entities]
        except KeyError:
            raise ValueError("entity is not in the document") from None

    def remove_all(self, entities: list[Entity]) -> None:
        """Remove many entities in one pass. Removing them one by one rescans the drawing for each."""
        doomed = {id(entity) for entity in entities}
        self.entities = [entity for entity in self.entities if id(entity) not in doomed]

    def layer_of(self, entity: Entity) -> Layer:
        return self.layers.get(entity.layer) or self.layers.get(DEFAULT_LAYER_NAME) or Layer(DEFAULT_LAYER_NAME)

    def effective_linetype(self, entity: Entity) -> str:
        return self.layer_of(entity).linetype if entity.linetype == BYLAYER_LINETYPE else entity.linetype

    def is_visible(self, entity: Entity) -> bool:
        return self.layer_of(entity).visible

    def visible_entities(self) -> list[Entity]:
        """Everything on a layer that is turned on. This is what can be seen and snapped to."""
        return [entity for entity in self.entities if self.is_visible(entity)]

    def is_editable(self, entity: Entity) -> bool:
        layer = self.layer_of(entity)
        return layer.visible and not layer.locked

    def editable_entities(self) -> list[Entity]:
        """Everything that can be picked and changed: visible and not on a locked layer."""
        return [entity for entity in self.entities if self.is_editable(entity)]

    def layer_in_use(self, name: str) -> bool:
        return any(entity.layer == name for entity in self.entities)
