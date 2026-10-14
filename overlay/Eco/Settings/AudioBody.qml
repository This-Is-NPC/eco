pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Layouts
import Eco.Core
import Eco.Kit

// AudioBody is the audio tab in place of a table: every device, with its live
// trace while it is heard, the audio source it is heard as and its colour, each
// chosen in its row; then the audio sources, named and marked as you in their
// rows. Every change saves at once. Enter or Space on a device opens its HEARD
// AS list, on a source starts editing its name; Tab reaches the row's controls.
ScrollPage {
  id: body

  required property Item settings
  required property SettingsSection section

  readonly property int at: settings.at.audio
  readonly property string picked: settings.picked[at] || ""
  readonly property var colors: Eco.config !== null && Eco.config.colors || ({})
  readonly property var sourceRows: Eco.config === null ? [] : section.rows(Eco.config)
  readonly property var deviceRows: Eco.config === null ? [] : section.deviceRows(Eco.config)
  readonly property var deviceColumns: [{ title: I18n.t("audio.column_device"), weight: 0.34 }, { title: I18n.t("audio.column_kind"), weight: 0.1 },
    { title: I18n.t("audio.column_level"), weight: 0.2 }, { title: I18n.t("audio.column_owner"), weight: 0.24 },
    { title: I18n.t("audio.column_color"), weight: 0.12 }]
  readonly property var sourceColumns: [{ title: I18n.t("settings.column_name"), weight: 0.7 }, { title: I18n.t("audio.column_you"), weight: 0.3 }]
  // What a table says while the config is still to come, or has nothing.
  readonly property string waiting: Eco.connected ? I18n.t("config.loading") : I18n.t("start.offline")

  contentHeight: content.implicitHeight

  // The keyboard goes to the table of the row picked.
  function takeFocus() {
    if (picked.startsWith("source:") && sources.rows.length > 0)
      sources.focusRows()
    else
      devices.focusRows()
  }
  function pick(id) { settings.picked = settings.setOf(settings.picked, at, id) }
  // The colour a device shows in: the one chosen, else the automatic one.
  function tone(id) { return colors[id] || Eco.inputColor(id) }
  // The room the columns `from` up to `to` of `table` take, as DataTable lays them out.
  function span(table, from, to) {
    let share = 0
    for (let i = from; i < to; i++)
      share += table.columns[i].weight
    return Math.max(0, table.width - 24) * share
  }
  // The HEARD AS option of the source a device is heard as, or "none".
  function heardAs(id) {
    const owner = Eco.config.participants.find(person => person.devices.includes(id))
    return owner ? section.sourceId(owner.name) : "none"
  }
  // The device `id` heard as `option`: a source's row id, "none" or "new".
  function hear(id, option) {
    if (option === "new") {
      naming.names = sourceRows.map(row => row.name)
      naming.ask(id, section.label(id))
      return
    }
    const row = sourceRows.find(entry => entry.id === option)
    const name = row ? row.name : ""
    settings.saveNow(d => Draft.assign(d, id, d.participants.findIndex(person => person.name === name)), () => pick(section.deviceId(id)))
  }

  ColumnLayout {
    id: content
    width: body.width
    spacing: 12

    Kicker { Layout.fillWidth: true; text: I18n.t("audio.devices") }
    DataTable {
      id: devices
      Layout.fillWidth: true
      Layout.preferredHeight: implicitHeight
      picks: true
      columns: body.deviceColumns
      stacked: [[0], [1]]
      rows: body.deviceRows
      cells: row => row.cells
      dimmed: row => row.missing
      tools: body.deviceTools
      // Wide, the controls lie over their own columns; stacked, they keep room on the right.
      toolsWidth: compact ? 230 : 0
      toolsAlways: true
      toolsLive: true
      selectedId: body.picked.startsWith("device:") ? body.picked : ""
      loading: Eco.config === null && Eco.connected
      emptyText: Eco.config === null ? body.waiting : I18n.t("audio.devices_empty")
      onPicked: row => body.pick(row.id)
      onBackRequested: body.settings.closeRequested()
    }

    Kicker { Layout.fillWidth: true; Layout.topMargin: 12; text: I18n.t("audio.sources") }
    DataTable {
      id: sources
      Layout.fillWidth: true
      Layout.preferredHeight: implicitHeight
      picks: true
      columns: body.sourceColumns
      stacked: [[0]]
      rows: body.sourceRows
      cells: row => row.cells
      tools: body.sourceTools
      toolsAlways: true
      toolsLive: true
      selectedId: body.picked.startsWith("source:") ? body.picked : ""
      emptyText: Eco.config === null ? body.waiting : I18n.t("audio.sources_empty")
      onPicked: row => body.pick(row.id)
      onRemoveRequested: row => body.settings.remove(body.at, row)
      onBackRequested: body.settings.closeRequested()
      // ↑ on the first source goes to the last device.
      onTopLeft: {
        if (body.deviceRows.length === 0)
          return
        body.pick(body.deviceRows[body.deviceRows.length - 1].id)
        devices.focusRows()
      }
    }
  }

  // A device's controls: its live trace while it is heard, the source it is
  // heard as and its colour. Enter or Space opens the list.
  property Component deviceTools: Item {
    id: controls
    property var entry: ({})
    readonly property string device: controls.entry.device || ""
    readonly property var heard: Eco.signals[controls.device]
    function act() { owner.open() }
    implicitWidth: line.width + list.width + swatch.width
    implicitHeight: 28

    Item {
      id: line
      width: devices.compact ? 66 : body.span(devices, 2, 3)
      height: parent.height
      EcgTrace {
        anchors { left: parent.left; right: parent.right; rightMargin: 12; verticalCenter: parent.verticalCenter }
        height: 22
        visible: controls.heard !== undefined
        glow: false
        source: controls.heard || null
        tone: body.tone(controls.device)
      }
    }
    Item {
      id: list
      x: line.width
      width: devices.compact ? owner.implicitWidth + 10 : body.span(devices, 3, 4)
      height: parent.height
      Dropdown {
        id: owner
        anchors.verticalCenter: parent.verticalCenter
        options: body.sourceRows.map(row => row.id).concat(["none", "new"])
        current: Eco.config !== null ? body.heardAs(controls.device) : "none"
        dim: current === "none"
        describe: option => option === "none" ? I18n.t("audio.not_heard") : option === "new" ? I18n.t("audio.new_source")
          : (body.sourceRows.find(row => row.id === option) || { name: "" }).name
        onPicked: option => body.hear(controls.device, option)
      }
    }
    Item {
      id: swatch
      x: line.width + list.width
      width: devices.compact ? 22 : body.span(devices, 4, 5) + 6
      height: parent.height
      ColorPicker {
        anchors.verticalCenter: parent.verticalCenter
        chosen: body.colors[controls.device] || ""
        automatic: Eco.inputColor(controls.device)
        onPicked: color => {
          const id = controls.device
          body.settings.saveNow(d => Draft.recolor(d, id, color), () => body.pick(body.section.deviceId(id)))
        }
      }
    }
  }

  // A source's controls: its name, kept on Enter or leaving it and given back
  // by Esc, and whether it is you. Enter or Space starts editing the name.
  property Component sourceTools: Item {
    id: source
    property var entry: ({})
    readonly property string saved: source.entry.name || ""
    function act() { box.act() }
    // Saves the name typed; one empty or another source's is refused, and the
    // saved one comes back.
    function commit() {
      const name = box.text.trim()
      const from = source.saved
      if (name === from) {
        box.text = from
        return
      }
      const problem = name === "" ? "settings.error_empty" : body.sourceRows.some(row => row.name === name) ? "settings.error_taken" : ""
      if (problem !== "") {
        Eco.say(I18n.t(problem), true)
        box.text = from
        return
      }
      body.settings.saveNow(d => Draft.renameSource(d, from, name), () => body.pick(body.section.sourceId(name)))
    }
    implicitWidth: sources.width - 18
    implicitHeight: 30

    SyncedBox {
      id: box
      anchors.verticalCenter: parent.verticalCenter
      width: body.span(sources, 0, 1) - 16
      dense: true
      inline: true
      value: source.saved
      onEditingFinished: source.commit()
    }
    Chip {
      x: body.span(sources, 0, 1)
      anchors.verticalCenter: parent.verticalCenter
      text: body.settings.onOff(source.entry.user).toUpperCase()
      checked: source.entry.user === true
      dim: !checked
      tip: I18n.t("audio.you_tip")
      onClicked: {
        const name = source.saved
        body.settings.saveNow(d => Draft.makeUser(d, d.participants.findIndex(person => person.name === name)), () => body.pick(body.section.sourceId(name)))
      }
    }
    // A rename the daemon refused shows the saved name again.
    Connections {
      target: Eco
      function onConfigSaveFailed() { box.text = source.saved }
    }
  }

  NewSourceDialog {
    id: naming
    onNamed: (id, name) => body.settings.saveNow(d => Draft.addSource(d, name, id), () => {
      body.pick(body.section.deviceId(id))
      devices.focusRows()
    })
  }
}
