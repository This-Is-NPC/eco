pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Layouts
import Eco.Core
import Eco.Kit

// AudioPage says who is heard through which device: an audio source (a
// `participants` entry in the config) is a name in the conversation, heard
// through any number of devices; a device has one owner. Each audio source is
// a card with its devices, and one may be the user;
// removing one first says what stops being heard.
ColumnLayout {
  id: page

  required property var participants
  required property var devices
  // Device id -> "#rrggbb" chosen by the user.
  required property var colors
  // How the user's microphone is kept apart from the others ({echo_cancel, drop_echoes}).
  required property var audio
  // Messages by field path ("participants.<index>"), as the settings window shows them.
  property var errors: ({})

  signal changed(var participants)
  signal recolored(var colors)
  signal audioEdited(var audio)

  spacing: 16

  function copy() { return JSON.parse(JSON.stringify(page.participants)) }

  function setAudio(key, value) {
    const audio = Object.assign({ echo_cancel: false, drop_echoes: true }, page.audio)
    audio[key] = value
    page.audioEdited(audio)
  }

  // Gives the keyboard to the name of the audio source at `path`.
  function reveal(path) {
    const [list, at] = path.split(".")
    if (list === "participants")
      Qt.callLater(() => sources.itemAt(Number(at)).nameBox.forceActiveFocus())
  }

  function device(id) { return page.devices.find(device => device.id === id) || null }

  // What a device is called, with its kind, and who has it when it is someone else.
  function deviceName(id, except) {
    const found = page.device(id)
    const name = found ? Eco.deviceLabel(id, found.label) : I18n.t("settings.device_missing", { id: id })
    const kind = found ? I18n.t(found.kind === "input" ? "audio.kind_input" : "audio.kind_output") + "  " : ""
    const owner = page.participants.find((p, i) => i !== except && p.devices.includes(id))
    return kind + name + (owner ? "  ·  " + owner.name : "")
  }

  // Give `id` to the audio source at `index`, taking it from whoever had it.
  function assign(id, index) {
    const participants = page.copy()
    participants.forEach(p => p.devices = p.devices.filter(device => device !== id))
    if (index >= 0)
      participants[index].devices.push(id)
    page.changed(participants)
  }

  Panel {
    Layout.fillWidth: true
    Layout.preferredHeight: implicitHeight
    index: "01"
    title: I18n.t("audio.participants")
    tools: Chip {
      text: I18n.t("audio.refresh")
      tip: I18n.t("audio.refresh_tip")
      onClicked: Eco.requestDevices()
    }

    ColumnLayout {
      anchors.fill: parent
      spacing: 10

      Repeater {
        id: sources
        model: page.participants.length
        delegate: SurfaceFrame {
          id: card
          required property int index
          readonly property alias nameBox: nameBox
          readonly property string error: page.errors["participants." + index] || ""
          // Past the list for a moment while a removal shrinks it.
          readonly property var participant: page.participants[index] || ({ name: "", devices: [], user: false })
          Layout.fillWidth: true
          implicitHeight: body.implicitHeight + 24
          active: participant.user
          tone: Theme.participantColor(index)

          ColumnLayout {
            id: body
            anchors { left: parent.left; right: parent.right; top: parent.top; margins: 12 }
            spacing: 8

            RowLayout {
              Layout.fillWidth: true
              spacing: 8
              Dot { size: 7; tone: Theme.participantColor(card.index) }
              SyncedBox {
                id: nameBox
                Layout.fillWidth: true
                dense: true
                invalid: card.error !== ""
                value: card.participant.name
                onTextEdited: {
                  const participants = page.copy()
                  participants[card.index].name = text
                  page.changed(participants)
                }
              }
              Chip {
                text: card.participant.user ? I18n.t("audio.you") : I18n.t("audio.make_you")
                checked: card.participant.user
                dim: !checked
                accent: Theme.participantColor(card.index)
                tip: I18n.t("audio.you_tip")
                onClicked: {
                  const participants = page.copy()
                  participants.forEach((p, i) => p.user = i === card.index && !card.participant.user)
                  page.changed(participants)
                }
              }
              IconButton {
                name: "close"
                tone: Theme.error
                tip: I18n.t("audio.remove_tip")
                onClicked: remove.ask(card.index)
              }
            }

            FieldError { Layout.fillWidth: true; text: card.error }

            Label {
              visible: card.participant.devices.length === 0
              text: I18n.t("settings.no_device")
              color: Theme.warning
              font.pixelSize: 10
            }

            Repeater {
              model: card.participant.devices
              delegate: RowLayout {
                id: line
                required property string modelData
                readonly property var found: page.device(modelData)
                Layout.fillWidth: true
                spacing: 10
                ColorPicker {
                  chosen: page.colors[line.modelData] || ""
                  automatic: Eco.inputColor(line.modelData)
                  onPicked: color => {
                    const colors = Object.assign({}, page.colors)
                    if (color)
                      colors[line.modelData] = color
                    else
                      delete colors[line.modelData]
                    page.recolored(colors)
                  }
                }
                Label {
                  Layout.fillWidth: true
                  text: page.deviceName(line.modelData, card.index)
                  color: line.found ? Theme.foreground : Theme.warning
                  font.pixelSize: 11
                  elide: Text.ElideRight
                }
                // A device being captured shows its live trace, in its colour.
                EcgTrace {
                  Layout.preferredWidth: Math.min(120, card.width / 4)
                  Layout.preferredHeight: 22
                  glow: false
                  visible: Eco.signals[line.modelData] !== undefined
                  source: Eco.signals[line.modelData] || null
                  tone: page.colors[line.modelData] || Eco.inputColor(line.modelData)
                }
                IconButton {
                  name: "close"
                  tip: I18n.t("audio.unassign_tip")
                  onClicked: page.assign(line.modelData, -1)
                }
              }
            }

            Dropdown {
              options: page.devices.map(device => device.id).filter(id => !card.participant.devices.includes(id))
              visible: options.length > 0
              current: ""
              label: I18n.t("audio.add_device")
              describe: id => page.deviceName(id, card.index)
              onPicked: id => page.assign(id, card.index)
            }
          }
        }
      }

      RowLayout {
        Layout.fillWidth: true
        spacing: 6
        TextBox {
          id: newParticipant
          Layout.fillWidth: true
          dense: true
          placeholderText: I18n.t("audio.new_placeholder")
          onAccepted: add.clicked()
        }
        IconButton {
          id: add
          name: "plus"
          tip: I18n.t("audio.add_tip")
          onClicked: {
            if (newParticipant.text.trim().length === 0)
              return
            const participants = page.copy()
            participants.push({ name: newParticipant.text.trim(), devices: [], user: false })
            newParticipant.text = ""
            page.changed(participants)
          }
        }
      }
    }
  }

  ConfirmDialog {
    id: remove
    property int index: -1
    property string name: ""
    property var consequences: []
    // Asks before removing the audio source at `index`, saying which devices go
    // unheard, whether nobody else is heard any more and whether nobody is the user.
    function ask(index) {
      const participant = page.participants[index]
      const outputs = p => p.devices.some(id => { const found = page.device(id); return found && found.kind === "output" })
      const lost = []
      if (participant.devices.length)
        lost.push(I18n.t("audio.remove_unheard", { devices: participant.devices.map(id => {
          const found = page.device(id)
          return found ? Eco.deviceLabel(id, found.label) : id
        }).join(", ") }))
      if (outputs(participant) && !page.participants.some((p, i) => i !== index && outputs(p)))
        lost.push(I18n.t("audio.remove_no_output"))
      if (participant.user)
        lost.push(I18n.t("audio.remove_no_user"))
      remove.index = index
      remove.name = participant.name
      remove.consequences = lost
      remove.open()
    }
    title: I18n.t("audio.remove_title")
    question: I18n.t("audio.remove_question", { name: name })
    warning: consequences.join("\n")
    confirmText: I18n.t("settings.remove_confirm")
    onConfirmed: {
      const participants = page.copy()
      participants.splice(remove.index, 1)
      page.changed(participants)
    }
  }

  Panel {
    Layout.fillWidth: true
    Layout.preferredHeight: implicitHeight
    index: "02"
    title: I18n.t("audio.echo")

    ColumnLayout {
      anchors.fill: parent
      spacing: 8
      Repeater {
        model: [["drop_echoes", page.audio.drop_echoes !== false], ["echo_cancel", page.audio.echo_cancel === true]]
        delegate: RowLayout {
          id: option
          required property var modelData
          Layout.fillWidth: true
          spacing: 10
          Chip {
            text: I18n.t("audio." + option.modelData[0])
            checked: option.modelData[1]
            dim: !checked
            onClicked: page.setAudio(option.modelData[0], !option.modelData[1])
          }
          Label {
            Layout.fillWidth: true
            text: I18n.t("audio." + option.modelData[0] + "_tip")
            color: Theme.dim
            font.pixelSize: 10
            wrapMode: Text.Wrap
          }
        }
      }
    }
  }

  Item { Layout.fillHeight: true }
}
