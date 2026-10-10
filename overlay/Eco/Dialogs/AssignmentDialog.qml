pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Layouts
import Eco.Core
import Eco.Kit

// AssignmentDialog chooses a person for one speaker of a session or, when
// opened on one of its lines, for that line alone.
ModalDialog {
  id: dialog

  property string sessionId
  property string label
  property string speakerName
  property string scope: "speaker"
  // The line picked, by its time, or 0 for none.
  property double at: 0
  readonly property int affected: scope === "speaker" ? Eco.countOf(sessionId, "speech", label) : 1
  // What the daemon says of this speaker, or null until it has.
  readonly property var info: Eco.speakersOf(sessionId).find(s => s.label === label) || null
  property var others: []
  // The known person picked, or "".
  property string chosen: ""
  property string colorChoice: ""
  property bool colorTouched: false
  readonly property string currentColor: {
    const id = chosen || (info && info.person ? info.person : "")
    const person = Eco.people.find(p => p.id === id)
    return person ? person.color || "" : info ? info.color || "" : ""
  }

  readonly property bool hasVoice: info !== null && info.voice
  readonly property var suggested: hasVoice ? info.suggestions : []
  // Everyone else known, after the people the voice suggests.
  readonly property var known: Eco.people.filter(p =>
    !suggested.some(s => s.person === p.id))

  maxWidth: 480

  function edit(speaker) {
    sessionId = speaker.session
    label = speaker.label || ""
    speakerName = speaker.name || ""
    scope = speaker.scope || "speaker"
    at = speaker.at || 0
    chosen = ""
    colorChoice = ""
    colorTouched = false
    nameBox.text = scope === "speaker" ? speakerName : ""
    Eco.requestSpeakers(speaker.session)
    Eco.send("people")
    // Speakers who are already people are offered among the known ones.
    const known = name => Eco.people.some(p => p.name === name)
    others = Eco.speakerNames(speaker.session).filter(name => name !== speaker.name && !known(name))
    open()
  }

  function pick(person, name) {
    chosen = person
    nameBox.text = name
    if (person) {
      colorChoice = ""
      colorTouched = false
    }
  }

  function save() {
    const name = nameBox.text.trim()
    if (scope === "line") {
      if (chosen === "" && name === "")
        return
      Eco.assignLine(sessionId, label, at, chosen, name)
    } else if (chosen !== "")
      Eco.assignPerson(sessionId, label, chosen, "", colorTouched ? colorChoice : null)
    else if (name !== "" && name !== speakerName && name !== label)
      Eco.assignPerson(sessionId, label, "", name, colorTouched ? colorChoice : null)
    else if (info && info.person && colorTouched)
      Eco.setPersonColor(info.person, colorChoice)
    else if (colorTouched)
      Eco.setSpeakerColor(sessionId, label, colorChoice)
    close()
  }

  onOpened: nameBox.forceActiveFocus()

  contentItem: DialogPanel {
    dialogOpen: dialog.visible
    implicitHeight: form.implicitHeight + 56
    title: I18n.t("assign.title")
    active: true

    ColumnLayout {
      id: form
      anchors.fill: parent
      spacing: 18

      ColumnLayout {
        Layout.fillWidth: true
        spacing: 2
        Caption { text: I18n.t("assign.scope") }
        Flow {
          Layout.fillWidth: true
          spacing: 6
          Chip {
            visible: dialog.at > 0
            text: I18n.t("assign.line")
            checked: dialog.scope === "line"
            onClicked: { dialog.scope = "line"; nameBox.text = ""; dialog.chosen = "" }
          }
          Chip {
            text: I18n.t("assign.speaker")
            checked: dialog.scope === "speaker"
            onClicked: { dialog.scope = "speaker"; nameBox.text = dialog.speakerName; dialog.chosen = "" }
          }
        }
        // Many lines changing at once is worth a warning; one is not.
        Label {
          text: I18n.t("assign.affected", { n: dialog.affected })
          color: dialog.affected > 1 ? Theme.warning : Theme.dim
          font.pixelSize: 10
        }
      }

      ColumnLayout {
        Layout.fillWidth: true
        spacing: 2
        Caption { text: I18n.t("speaker.person") }
        TextBox {
          id: nameBox
          Layout.fillWidth: true
          placeholderText: dialog.label
          onTextEdited: { dialog.chosen = ""; dialog.colorChoice = ""; dialog.colorTouched = false }
          onAccepted: dialog.save()
        }
      }

      RowLayout {
        Layout.fillWidth: true
        visible: dialog.scope === "speaker"
        spacing: 10
        ColorPicker {
          chosen: dialog.colorTouched ? dialog.colorChoice : dialog.currentColor
          automatic: Eco.speakerColor(dialog.sessionId, dialog.label, dialog.speakerName)
          onPicked: color => { dialog.colorChoice = color; dialog.colorTouched = true }
        }
        Label { text: I18n.t("speaker.color"); color: Theme.dim; font.pixelSize: 10; font.letterSpacing: 2 }
        Item { Layout.fillWidth: true }
      }

      ColumnLayout {
        Layout.fillWidth: true
        visible: dialog.suggested.length + dialog.known.length > 0
        spacing: 6
        Caption { text: I18n.t("speaker.known") }
        Flow {
          Layout.fillWidth: true
          spacing: 6
          Repeater {
            model: dialog.suggested
            delegate: Chip {
              required property var modelData
              text: I18n.t("speaker.suggestion", { name: modelData.name, score: Math.round(modelData.score * 100) })
              accent: Eco.personColor(modelData.person)
              checked: dialog.chosen === modelData.person
              dim: !checked
              onClicked: dialog.pick(modelData.person, modelData.name)
            }
          }
          Repeater {
            model: dialog.known
            delegate: Chip {
              required property var modelData
              text: modelData.name
              accent: Eco.personColor(modelData.id)
              checked: dialog.chosen === modelData.id
              dim: !checked
              onClicked: dialog.pick(modelData.id, modelData.name)
            }
          }
        }
      }

      ColumnLayout {
        Layout.fillWidth: true
        visible: dialog.scope === "speaker" && dialog.others.length > 0
        spacing: 6
        Caption { text: I18n.t("speaker.same_as") }
        Flow {
          Layout.fillWidth: true
          spacing: 6
          Repeater {
            model: dialog.others
            delegate: Chip {
              required property string modelData
              text: modelData
              accent: Eco.identityColor(dialog.sessionId + ":" + modelData, "")
              checked: nameBox.text.trim() === modelData
              dim: !checked
              onClicked: dialog.pick("", modelData)
            }
          }
        }
      }

      DialogFooter {
        primaryIcon: "enter"
        primaryText: I18n.t("rename.save")
        onCancelled: dialog.close()
        onAccepted: dialog.save()
        Chip {
          visible: dialog.scope === "speaker" && dialog.info !== null && !!dialog.info.person
          icon: "close"
          text: I18n.t("speaker.not_them")
          dim: true
          tip: I18n.t("speaker.not_them_tip")
          onClicked: {
            Eco.unassignPerson(dialog.sessionId, dialog.label)
            dialog.close()
          }
        }
      }
    }
  }
}
