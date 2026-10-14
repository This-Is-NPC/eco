pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Layouts
import Eco.Core
import Eco.Kit

// SourceView is one audio source: a name in the conversation, heard through
// its devices. Read, whether it is you and its devices, each with its kind,
// its colour and its live trace while captured. Edited: its name, whether it
// is you (one source at most is), and its devices and their colours; a device
// added here is taken from the source that had it.
ItemPage {
  id: page

  required property Item settings
  required property SettingsSection section

  readonly property bool fresh: key === "+"
  readonly property var participants: config ? config.participants : []
  // Its place in the config shown: past the end of the saved list for a new one.
  readonly property int at: item.active ? (fresh ? item.base.participants.length : item.base.participants.findIndex(person => section.sourceId(person.name) === key))
    : participants.findIndex(person => section.sourceId(person.name) === key)
  readonly property var entry: participants[at] || ({ name: "", devices: [], user: false })
  readonly property var colors: config && config.colors || ({})
  readonly property string error: item.errors["participants." + at] || ""

  function recolor(id, color) {
    item.update(d => {
      d.colors = d.colors || {}
      if (color)
        d.colors[id] = color
      else
        delete d.colors[id]
    })
  }

  kind: I18n.t("audio.item_source")
  title: entry.name
  placeholder: I18n.t("audio.source_new")
  nextKey: section.sourceId(entry.name.trim())
  draft: ItemDraft { id: item; paths: page.at >= 0 ? ["participants." + page.at] : [] }
  Component.onCompleted: if (fresh) item.begin(d => d.participants.push({ name: "", devices: [], user: false }))

  facts: [[I18n.t("audio.column_you"), I18n.t(entry.user ? "audio.is_you" : "audio.not_you")]]
  details: Loader { Layout.fillWidth: true; active: !page.editing; sourceComponent: page.devices }

  // Its devices, read or edited: each with its colour, chosen while edited,
  // and its live trace while captured; edited, a device is added or taken off.
  property Component devices: ColumnLayout {
    spacing: 8
    Caption { text: I18n.t("audio.column_devices").toUpperCase() }
    Label {
      visible: page.entry.devices.length === 0
      text: I18n.t("settings.no_device")
      color: Theme.warning
      font.pixelSize: 10
    }
    Repeater {
      model: page.entry.devices
      delegate: RowLayout {
        id: line
        required property string modelData
        readonly property color tone: page.colors[line.modelData] || Eco.inputColor(line.modelData)
        Layout.fillWidth: true
        spacing: 10
        ColorPicker {
          visible: page.editing
          chosen: page.colors[line.modelData] || ""
          automatic: Eco.inputColor(line.modelData)
          onPicked: color => page.recolor(line.modelData, color)
        }
        Dot { visible: !page.editing; size: 8; tone: line.tone }
        Label {
          Layout.fillWidth: true
          text: page.section.deviceName(page.participants, line.modelData, page.at)
          color: page.section.device(line.modelData) ? Theme.foreground : Theme.warning
          font.pixelSize: 11
          elide: Text.ElideRight
        }
        EcgTrace {
          Layout.preferredWidth: Math.min(120, line.width / 4)
          Layout.preferredHeight: 22
          glow: false
          visible: Eco.signals[line.modelData] !== undefined
          source: Eco.signals[line.modelData] || null
          tone: line.tone
        }
        IconButton {
          visible: page.editing
          name: "close"
          tip: I18n.t("audio.unassign_tip")
          onClicked: {
            const id = line.modelData
            item.update(d => Draft.assign(d, id, -1))
          }
        }
      }
    }
    Dropdown {
      options: Eco.devices.map(device => device.id).filter(id => !page.entry.devices.includes(id))
      visible: page.editing && options.length > 0
      current: ""
      label: I18n.t("audio.add_device")
      describe: id => page.section.deviceName(page.participants, id, page.at)
      onPicked: id => {
        const at = page.at
        item.update(d => Draft.assign(d, id, at))
      }
    }
  }

  Field {
    Layout.fillWidth: true
    label: I18n.t("settings.column_name")
    value: page.entry.name
    placeholder: I18n.t("audio.new_placeholder")
    error: page.error
    onEdited: text => {
      const at = page.at
      item.update(d => d.participants[at].name = text)
    }
  }

  RowLayout {
    Layout.fillWidth: true
    spacing: 10
    Chip {
      text: page.entry.user ? I18n.t("audio.you") : I18n.t("audio.make_you")
      checked: page.entry.user
      dim: !checked
      accent: Theme.participantColor(page.at)
      onClicked: {
        const at = page.at
        item.update(d => Draft.makeUser(d, at))
      }
    }
    Label {
      Layout.fillWidth: true
      text: I18n.t("audio.you_tip")
      color: Theme.dim
      font.pixelSize: 10
      wrapMode: Text.Wrap
    }
  }

  Loader { Layout.fillWidth: true; active: page.editing; sourceComponent: page.devices }
}
