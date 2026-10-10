import QtQuick

// ActionMenu keeps a view's less frequent actions behind one "more" button,
// opening a list aligned to its right edge. `items` are {id, icon, text,
// guarded}; `chosen` names the one picked.
Item {
  id: menu

  property var items: []
  property string tip
  signal chosen(string id)

  implicitWidth: button.implicitWidth
  implicitHeight: button.implicitHeight

  IconButton {
    id: button
    name: "more"
    tip: menu.tip
    highlighted: list.opened
    onClicked: list.opened ? list.close() : list.open()
  }

  MenuPopup {
    id: list
    alignRight: true
    entries: menu.items
    onPicked: index => menu.chosen(menu.items[index].id)
  }
}
