pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Layouts

// TagInput edits a list of tags: a chip per tag, which takes it off, and a field
// that adds one. While it is typed in, the known tags holding the text — those
// starting with it first — line up below: Tab completes the first, ↓ reaches
// them. A tag known in another case is added as it is known.
ColumnLayout {
  id: input

  property var tags: []
  property var known: []
  signal added(string tag)
  signal removed(string tag)

  spacing: 6

  readonly property string typed: box.text.trim().replace(/\s+/g, " ")
  readonly property var offered: {
    const text = typed.toLowerCase()
    const free = known.filter(tag => !tags.some(kept => Eco.sameTag(kept, tag)) && tag.toLowerCase().includes(text))
    return free.filter(tag => tag.toLowerCase().startsWith(text))
      .concat(free.filter(tag => !tag.toLowerCase().startsWith(text)))
  }

  function add(text) {
    const clean = text.trim().replace(/\s+/g, " ")
    if (!clean)
      return
    const tag = known.find(kept => Eco.sameTag(kept, clean)) || clean
    if (!tags.some(kept => Eco.sameTag(kept, tag)))
      added(tag)
    box.text = ""
    box.forceActiveFocus()
  }

  Flow {
    Layout.fillWidth: true
    visible: input.tags.length > 0
    spacing: 6
    Repeater {
      model: input.tags
      delegate: Chip {
        required property string modelData
        text: I18n.t("tags.removable", { tag: modelData })
        checked: true
        tip: I18n.t("tags.remove")
        onClicked: input.removed(modelData)
      }
    }
  }

  RowLayout {
    Layout.fillWidth: true
    spacing: 6
    TextBox {
      id: box
      Layout.fillWidth: true
      dense: true
      placeholderText: I18n.t("tags.placeholder")
      onAccepted: input.add(text)
      Keys.onTabPressed: event => {
        const first = input.offered[0]
        if (input.typed && first && first !== input.typed)
          box.text = first
        else
          event.accepted = false
      }
      Keys.onDownPressed: event => {
        if (offers.visible)
          offerList.itemAt(0).forceActiveFocus()
        else
          event.accepted = false
      }
    }
    Chip {
      icon: "plus"
      text: I18n.t("tags.add")
      enabled: input.typed.length > 0
      onClicked: input.add(box.text)
    }
  }

  ChipStrip {
    id: offers
    Layout.fillWidth: true
    visible: input.offered.length > 0 && (box.activeFocus || input.typed !== "" || holdsFocus)
    Repeater {
      id: offerList
      model: input.offered
      delegate: Chip {
        required property string modelData
        text: I18n.t("tags.label", { tag: modelData })
        dim: true
        onClicked: input.add(modelData)
      }
    }
  }
}
