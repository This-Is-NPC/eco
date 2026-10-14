pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Layouts
import Eco.Core
import Eco.Kit

// SourceView is one audio source: a name in the conversation, heard through
// its devices. Read, its name and whether it is you, the live trace of each
// device captured, and its devices, a child table whose rows are added (taken
// from the source that had one), removed and given a colour, each saved at
// once. Edited, its name and whether it is you (one source at most is).
ItemPage {
  id: page

  required property Item settings
  required property SettingsSection section

  readonly property bool fresh: key === "+"
  readonly property var participants: config ? config.participants : []
  // Its place in the config shown: past the end of the saved list for a new one.
  readonly property int at: item.active && item.base !== null ? (fresh ? item.base.participants.length : item.base.participants.findIndex(person => section.sourceId(person.name) === key))
    : participants.findIndex(person => section.sourceId(person.name) === key)
  readonly property var entry: participants[at] || ({ name: "", devices: [], user: false })
  readonly property var colors: config && config.colors || ({})
  readonly property string error: item.errors["participants." + at] || ""

  // The colour a device shows in: the one chosen, else the automatic one.
  function tone(id) { return colors[id] || Eco.inputColor(id) }
  // A device's kind as read, or that it is not there.
  function kindOf(id) {
    const found = section.device(id)
    return found ? I18n.t(found.kind === "input" ? "audio.kind_input" : "audio.kind_output") : I18n.t("audio.unavailable")
  }
  // A device's name, its id when it is not there.
  function nameOf(id) {
    const found = section.device(id)
    return found ? Eco.deviceLabel(id, found.label) : id
  }
  // Where the source named `name` is in `d`, a copy of the saved config.
  function placeOf(d, name) { return d.participants.findIndex(person => person.name === name) }

  kind: I18n.t("audio.item_source")
  title: entry.name
  placeholder: I18n.t("audio.source_new")
  nextKey: section.sourceId(entry.name.trim())
  draft: ItemDraft { id: item; paths: page.at >= 0 ? ["participants." + page.at] : [] }
  Component.onCompleted: if (fresh) item.begin(d => d.participants.push({ name: "", devices: [], user: false }))

  facts: [[I18n.t("settings.column_name"), entry.name], [I18n.t("audio.column_you"), I18n.t(entry.user ? "audio.is_you" : "audio.not_you")]]
  // The devices captured now, each with its live trace.
  details: ColumnLayout {
    id: live
    readonly property var heard: page.entry.devices.filter(id => Eco.signals[id] !== undefined)
    Layout.fillWidth: true
    visible: heard.length > 0
    spacing: 8
    Caption { text: I18n.t("audio.live").toUpperCase() }
    Repeater {
      model: live.heard
      delegate: RowLayout {
        id: line
        required property string modelData
        Layout.fillWidth: true
        spacing: 10
        Dot { size: 8; tone: page.tone(line.modelData) }
        Label {
          Layout.fillWidth: true
          text: page.nameOf(line.modelData)
          font.pixelSize: 11
          elide: Text.ElideRight
        }
        EcgTrace {
          Layout.preferredWidth: Math.min(160, line.width / 3)
          Layout.preferredHeight: 22
          glow: false
          source: Eco.signals[line.modelData] || null
          tone: page.tone(line.modelData)
        }
      }
    }
  }

  tables: [{
    name: "devices",
    title: I18n.t("audio.column_devices"),
    columns: [{ title: I18n.t("settings.column_name"), weight: 0.6 }, { title: I18n.t("audio.column_kind"), weight: 0.15 },
      { title: I18n.t("audio.column_color"), weight: 0.25 }],
    stacked: [[0], [1, 2]],
    // Its swatch in its colour; a device not there says so.
    tone: (entry, column) => column === 2 ? page.tone(entry.id) : page.section.device(entry.id) ? "" : Theme.warning,
    rows: entry.devices.map(id => ({ id: id, cells: [page.nameOf(id), page.kindOf(id), "■  " + (page.colors[id] || I18n.t("color.auto"))] })),
    empty: I18n.t("audio.devices_empty"),
    add: { text: I18n.t("audio.add_device"), tip: I18n.t("audio.add_device_tip"), screen: addDevice },
    remove: row => {
      const name = page.entry.name
      const left = Draft.unassignment(Eco.config, name, row.id, Eco.devices)
      const asked = left.empty || left.noOutput
      return {
        title: I18n.t("audio.unassign_title"),
        question: asked ? I18n.t("audio.unassign_question", { name: name, device: page.nameOf(row.id) }) : "",
        warning: (left.empty ? [I18n.t("audio.unassign_empty", { name: name })] : []).concat(left.noOutput ? [I18n.t("audio.remove_no_output")] : []).join("\n"),
        change: d => Draft.assign(d, row.id, -1)
      }
    },
    open: { text: I18n.t("audio.device_color"), tip: I18n.t("audio.device_color_tip"), screen: deviceColor }
  }]

  // The devices this source is not heard through, each with its kind and the
  // source that has it, to add one.
  property Component addDevice: ChildScreen {
    title: I18n.t("audio.add_device")
    note: I18n.t("audio.add_device_note")
    columns: [{ title: I18n.t("settings.column_name"), weight: 0.55 }, { title: I18n.t("audio.column_kind"), weight: 0.15 },
      { title: I18n.t("audio.column_owner"), weight: 0.3 }]
    stacked: [[0], [1, 2]]
    rows: Eco.devices.filter(device => !page.entry.devices.includes(device.id)).map(device => {
      const owner = Eco.config.participants.find(person => person.devices.includes(device.id))
      return { id: device.id, cells: [page.nameOf(device.id), page.kindOf(device.id), owner ? owner.name : "—"] }
    })
    empty: I18n.t("audio.add_device_empty")
    chooseText: I18n.t("audio.add_device")
    chooseTip: I18n.t("audio.add_device_choose_tip")
    choose: row => {
      const name = page.entry.name
      return { change: d => Draft.assign(d, row.id, page.placeOf(d, name)), pick: row.id }
    }
  }

  // A device's own screen: the colour it shows in.
  property Component deviceColor: ChildScreen {
    id: coloring
    readonly property string saved: row ? page.colors[row.id] || "" : ""
    property string chosen: saved
    title: row ? page.nameOf(row.id) : ""
    note: I18n.t("audio.device_color_note")
    commands: [
      { text: I18n.t("config.save"), icon: "enter", role: "primary", tip: I18n.t("audio.color_save_tip"),
        enabled: coloring.chosen !== coloring.saved && !Eco.configSaving,
        run: () => {
          const id = coloring.row.id
          const color = coloring.chosen
          coloring.finish({ change: d => Draft.recolor(d, id, color), pick: id })
        } }
    ]
    RowLayout {
      spacing: 12
      Caption { text: I18n.t("audio.column_color").toUpperCase() }
      ColorPicker {
        // Under it, the screen's note stays readable while it holds the keyboard.
        tipBelow: true
        chosen: coloring.chosen
        automatic: coloring.row ? Eco.inputColor(coloring.row.id) : Theme.dim
        onPicked: color => coloring.chosen = color
      }
      Label {
        text: coloring.chosen || I18n.t("color.auto")
        color: coloring.chosen || coloring.row && Eco.inputColor(coloring.row.id) || Theme.dim
        font.pixelSize: 11
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
}
