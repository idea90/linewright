import ezdxf
import pytest

from linewright.io.dxf import document_from_drawing, drawing_from_document
from linewright.io.files import open_drawing, save_drawing
from linewright.model.document import Document, Layer
from linewright.model.entities import Circle, Line


def reread(document, tmp_path, name="layers.dxf"):
    path = tmp_path / name
    save_drawing(document, path)
    return open_drawing(path)


def test_layer_properties_survive_a_save_and_open(tmp_path):
    document = Document(entities=[Line((0, 0), (1, 1), layer="Walls")])
    document.layers["Walls"] = Layer("Walls", color=1, linetype="Dashed", visible=False, locked=True)
    document.layers["Doors"] = Layer("Doors", color=5, linetype="Center")
    back = reread(document, tmp_path)
    assert back.layers["Walls"] == Layer("Walls", 1, "Dashed", visible=False, locked=True)
    assert back.layers["Doors"] == Layer("Doors", 5, "Center", visible=True, locked=False)
    assert "0" in back.layers


def test_a_hidden_layer_keeps_its_colour(tmp_path):
    document = Document()
    document.layers["Walls"] = Layer("Walls", color=3, visible=False)
    assert reread(document, tmp_path).layers["Walls"].color == 3


def test_entity_linetypes_are_saved_and_by_layer_stays_by_layer(tmp_path):
    document = Document(
        entities=[
            Line((0, 0), (1, 0), linetype="Hidden"),
            Line((0, 1), (1, 1)),
            Circle((0, 0), 1, linetype="Phantom", color=2),
        ]
    )
    back = reread(document, tmp_path)
    assert [e.linetype for e in back.entities] == ["Hidden", "ByLayer", "Phantom"]
    assert back.entities[2].color == 2


def test_linetypes_missing_from_the_file_are_added(tmp_path):
    document = Document(entities=[Line((0, 0), (1, 0), linetype="Dashed")])
    drawing = drawing_from_document(document)
    assert any(entry.dxf.name.lower() == "dashed" for entry in drawing.linetypes)


def test_layer_names_in_any_case_find_their_layer():
    source = ezdxf.new()
    source.layers.add("WALLS", color=1)
    source.modelspace().add_line((0, 0), (1, 1), dxfattribs={"layer": "walls"})
    document = document_from_drawing(source)
    assert document.entities[0].layer == "WALLS"
    assert document.layers["WALLS"].color == 1


def test_a_layer_used_but_never_defined_is_added():
    source = ezdxf.new()
    entity = source.modelspace().add_line((0, 0), (1, 1))
    entity.dxf.layer = "Ghost"
    document = document_from_drawing(source)
    assert "Ghost" in document.layers


def test_a_frozen_layer_reads_as_hidden_and_stays_frozen(tmp_path):
    source = ezdxf.new()
    source.layers.add("Cold", color=4).freeze()
    source.modelspace().add_line((0, 0), (1, 1), dxfattribs={"layer": "Cold"})
    path = tmp_path / "frozen.dxf"
    source.saveas(path)
    document = open_drawing(path)
    assert not document.layers["Cold"].visible
    save_drawing(document, path)
    assert ezdxf.readfile(path).layers.get("Cold").is_frozen()


def test_turning_a_frozen_layer_back_on_thaws_it(tmp_path):
    source = ezdxf.new()
    source.layers.add("Cold").freeze()
    path = tmp_path / "frozen.dxf"
    source.saveas(path)
    document = open_drawing(path)
    document.layers["Cold"].visible = True
    save_drawing(document, path)
    entry = ezdxf.readfile(path).layers.get("Cold")
    assert not entry.is_frozen() and entry.is_on()


def test_a_deleted_layer_is_removed_from_the_file_unless_something_still_uses_it(tmp_path):
    source = ezdxf.new()
    for name in ("Gone", "Kept", "Holds"):
        source.layers.add(name)
    source.modelspace().add_ellipse((0, 0), (5, 0), 0.5, dxfattribs={"layer": "Holds"})
    path = tmp_path / "purge.dxf"
    source.saveas(path)
    document = open_drawing(path)
    for name in ("Gone", "Holds"):
        del document.layers[name]
    save_drawing(document, path)
    names = {entry.dxf.name for entry in ezdxf.readfile(path).layers}
    assert "Gone" not in names
    assert {"Holds", "0"} <= names  # the ellipse we could not read still needs its layer


def test_layer_zero_and_defpoints_are_never_removed(tmp_path):
    source = ezdxf.new()
    path = tmp_path / "zero.dxf"
    source.saveas(path)
    document = open_drawing(path)
    document.layers.pop("Defpoints")
    save_drawing(document, path)
    assert {"0", "Defpoints"} <= {entry.dxf.name for entry in ezdxf.readfile(path).layers}


def test_saving_twice_does_not_duplicate_layers(tmp_path):
    document = Document()
    document.layers["Walls"] = Layer("Walls", 1)
    path = tmp_path / "twice.dxf"
    save_drawing(document, path)
    save_drawing(document, path)
    assert [e.dxf.name for e in ezdxf.readfile(path).layers].count("Walls") == 1


@pytest.mark.parametrize("name", ["Dashed", "dashed", "DASHED"])
def test_linetype_names_in_any_case_are_understood(name):
    from linewright.model.styles import canonical_linetype

    assert canonical_linetype(name) == "Dashed"
    assert canonical_linetype("BYLAYER") == "ByLayer"
    assert canonical_linetype("MyOwn") == "MyOwn"

