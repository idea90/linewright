from collections.abc import Iterable, Iterator

from linewright.model.entities import Entity


class Selection:
    """The selected entities. Membership is by identity: equal-looking entities are different objects."""

    def __init__(self) -> None:
        self._items: dict[int, Entity] = {}

    def __iter__(self) -> Iterator[Entity]:
        return iter(list(self._items.values()))

    def __len__(self) -> int:
        return len(self._items)

    def __contains__(self, entity: Entity) -> bool:
        return id(entity) in self._items

    def replace(self, entities: Iterable[Entity]) -> None:
        self._items = {id(e): e for e in entities}

    def add(self, entities: Iterable[Entity]) -> None:
        self._items.update({id(e): e for e in entities})

    def remove(self, entities: Iterable[Entity]) -> None:
        for entity in entities:
            self._items.pop(id(entity), None)

    def clear(self) -> None:
        self._items.clear()

    def keep_only(self, entities: Iterable[Entity]) -> None:
        """Forget anything that is no longer among the given entities."""
        alive = {id(e) for e in entities}
        self._items = {key: e for key, e in self._items.items() if key in alive}
