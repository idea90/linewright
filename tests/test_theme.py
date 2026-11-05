
from linewright.commands.registry import DRAWING, EDITING, shortest_alias
from linewright.theme import apply_theme
from linewright.ui.icons import command_icon




def test_stylesheet_parses_without_qt_warnings(qapp, qtlog):
    previous = qapp.styleSheet()
    apply_theme(qapp)
    qapp.setStyleSheet(previous)
    assert [r.message for r in qtlog.records if "stylesheet" in r.message.lower()] == []


def test_every_command_has_an_icon_in_each_state(qapp):
    for name in (*DRAWING, *EDITING):
        icon = command_icon(name)
        assert not icon.isNull()
        assert not icon.pixmap(24).isNull()


def test_tool_tooltips_show_the_short_name_to_type(window):
    tips = {name: action.toolTip() for name, action in window._tool_actions.items()}
    assert tips["LINE"] == "Line (L)"
    assert tips["PLINE"] == "Polyline (PL)"
    assert tips["RECTANGLE"] == "Rectangle (REC)"
    assert all(shortest_alias(name) for name in (*DRAWING, *EDITING))


def test_the_running_command_is_highlighted_in_the_tool_palette(window):
    window.controller.start_command("CIRCLE")
    assert [n for n, a in window._tool_actions.items() if a.isChecked()] == ["CIRCLE"]
    window.controller.start_command("ARC")
    assert [n for n, a in window._tool_actions.items() if a.isChecked()] == ["ARC"]
    window.controller.cancel()
    assert not any(a.isChecked() for a in window._tool_actions.values())


def test_clicking_the_running_tool_again_restarts_it_and_keeps_it_highlighted(window):
    action = window._tool_actions["LINE"]
    action.trigger()
    action.trigger()
    assert action.isChecked()
    assert window.controller._session is not None


