pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Controls.Basic as C
import QtQuick.Layouts
import Eco.Core
import Eco.Kit

Window {
  id: lab

  property bool ending: false
  property bool endBusy: false
  property bool paused: false
  property bool chipSelected: false
  property bool iconSelected: false
  property string section: "buttons"
  property string selectedOption: "first"
  property string pickedOption: "second"
  property string chosenColor: ""
  property string fieldValue: I18n.t("lab.sample_session")
  property int fieldNumber: 60000
  property var listItems: [I18n.t("lab.option_first"), I18n.t("lab.option_second")]
  property string lastAction: ""
  property string areaValue: I18n.t("lab.sample_prompt")
  property var labTags: [I18n.t("lab.sample_tag")]
  property string pickedTag: ""
  property string stripChoice: ""
  property var contextSlots: []

  Timer { id: labDisarm; interval: 3000; onTriggered: lab.ending = false }
  Timer { id: labEnd; interval: 1800; onTriggered: lab.endBusy = false }

  title: I18n.t("lab.window")
  width: 980
  height: 720
  minimumWidth: 680
  minimumHeight: 480
  color: Theme.background
  visible: true
  onClosing: Qt.quit()

  Rectangle {
    anchors.fill: parent
    color: Theme.background
    border.color: Theme.line

    Column {
      anchors { fill: parent; margins: 28 }
      spacing: 13

      Rectangle {
        z: 1
        width: parent.width
        height: header.implicitHeight
        color: Theme.background
        Column {
          id: header
          width: parent.width
          spacing: 13
          Label {
            text: I18n.t("lab.heading").toUpperCase()
            color: Theme.primary
            font.pixelSize: 18
            font.letterSpacing: 4
          }
          Label {
            width: parent.width
            text: I18n.t("lab.intro")
            color: Theme.dim
            font.pixelSize: 12
            wrapMode: Text.WordWrap
          }
          Flow {
            width: parent.width
            height: childrenRect.height
            spacing: 14
            Repeater {
              model: ["buttons", "selectors", "inputs", "feedback", "data", "surfaces", "motion", "icons"]
              delegate: Tab {
                required property string modelData
                required property int index
                label: I18n.t("lab.nav." + modelData)
                digit: String(index + 1).padStart(2, "0")
                current: lab.section === modelData
                onChosen: { lab.section = modelData; scroll.contentY = 0 }
              }
            }
          }
        }
      }

      Flickable {
        id: scroll
        width: parent.width
        height: parent.height - y
        contentWidth: width
        contentHeight: samples.implicitHeight + 20
        clip: true
        boundsBehavior: Flickable.StopAtBounds
        C.ScrollBar.vertical: C.ScrollBar {}

        Column {
          id: samples
          width: scroll.width
          spacing: 14

          Column {
            width: parent.width
            spacing: 14
            visible: lab.section === "buttons"

            ControlLabRow {
              width: parent.width
              title: I18n.t("lab.capsule_button")
              Rectangle {
                width: 380
                height: 54
                color: Qt.alpha(Theme.warning, 0.07)
                border.color: Qt.alpha(Theme.warning, 0.55)
                RowLayout {
                  anchors.fill: parent
                  spacing: 1
                  CapsuleButton {
                    icon: lab.paused ? "play" : "pause"
                    label: lab.paused ? I18n.t("session.resume") : I18n.t("session.pause")
                    onClicked: lab.paused = !lab.paused
                  }
                  CapsuleButton {
                    icon: "stop"
                    label: lab.ending ? I18n.t("session.end_confirm") : I18n.t("session.end")
                    tone: Theme.error
                    armed: lab.ending
                    busy: lab.endBusy
                    onClicked: {
                      if (lab.ending) {
                        lab.ending = false
                        labDisarm.stop()
                        lab.endBusy = true
                        labEnd.restart()
                      } else {
                        lab.ending = true
                        labDisarm.restart()
                      }
                    }
                  }
                }
              }
            }

            ControlLabRow {
              width: parent.width
              title: I18n.t("lab.trace_button")
              TraceButton { role: "primary"; icon: "plus"; text: I18n.t("start.begin") }
              TraceButton { role: "navigation"; icon: "list"; text: I18n.t("start.history") }
              TraceButton { role: "action"; icon: "import"; text: I18n.t("start.import") }
            }
            ControlLabRow {
              width: parent.width
              title: I18n.t("lab.chip")
              Chip {
                text: I18n.t("lab.option_first")
                checked: lab.chipSelected
                onClicked: lab.chipSelected = !lab.chipSelected
              }
              Chip { text: I18n.t("lab.option_second"); swatch: Theme.inputPalette[1]; accent: swatch; dim: true }
              Chip { text: I18n.t("lab.option_third"); reason: I18n.t("lab.chip_reason") }
            }
            ControlLabRow {
              width: parent.width
              title: I18n.t("lab.icon_button")
              IconButton {
                name: "edit"
                tip: I18n.t("lab.icon_button")
                highlighted: lab.iconSelected
                onClicked: lab.iconSelected = !lab.iconSelected
              }
              IconButton {
                name: "more"
                count: 3
                tip: I18n.t("guess.count", { n: 3 })
              }
            }
            ControlLabRow {
              width: parent.width
              title: I18n.t("lab.focus_ring")
              Row {
                spacing: 14
                Chip { text: I18n.t("lab.option_first"); checked: true; FocusRing { shown: true } }
                TraceButton { dense: true; text: I18n.t("lab.option_second"); FocusRing { shown: true } }
                Chip { text: I18n.t("keys.title"); icon: "list"; onClicked: labShortcuts.open() }
              }
            }
          }

          Column {
            width: parent.width
            spacing: 14
            visible: lab.section === "selectors"

            ControlLabRow {
              width: parent.width
              title: I18n.t("lab.dropdown")
              Row {
                spacing: 14
                Dropdown {
                  options: ["first", "second", "third"]
                  current: lab.selectedOption
                  emphasized: true
                  describe: option => option === "first" ? I18n.t("lab.option_first")
                    : option === "second" ? I18n.t("lab.option_second") : I18n.t("lab.option_third")
                  onPicked: option => lab.selectedOption = option
                }
                Dropdown {
                  options: ["first", "second"]
                  current: ""
                  label: I18n.t("settings.pick_model")
                  invalid: true
                  describe: option => option === "first" ? I18n.t("lab.option_first") : I18n.t("lab.option_second")
                }
              }
            }
            ControlLabRow {
              width: parent.width
              title: I18n.t("lab.language_picker")
              Row {
                spacing: 14
                LanguagePicker {}
                LanguagePicker { compact: true }
              }
            }
            ControlLabRow {
              width: parent.width
              title: I18n.t("lab.tab")
              Row {
                spacing: 20
                Tab { label: I18n.t("lab.option_first"); digit: "01"; current: true }
                Tab { label: I18n.t("lab.option_second"); digit: "02"; current: false; alert: true }
              }
            }
            ControlLabRow {
              width: parent.width
              title: I18n.t("lab.pick_row")
              Column {
                width: Math.min(460, parent.width)
                PickRow {
                  width: parent.width
                  text: I18n.t("lab.sample_label")
                  options: ["first", "second"]
                  current: lab.pickedOption
                  describe: option => option === "first" ? I18n.t("lab.option_first") : I18n.t("lab.option_second")
                  onPicked: option => lab.pickedOption = option
                }
                PickRow {
                  width: parent.width
                  text: I18n.t("lab.sample_label")
                  options: [""]
                  current: ""
                  describe: () => I18n.t("settings.default_model", { name: I18n.t("lab.option_first") })
                  dim: true
                }
              }
            }
            ControlLabRow {
              width: parent.width
              title: I18n.t("lab.action_menu")
              Row {
                spacing: 12
                ActionMenu {
                  tip: I18n.t("detail.more")
                  items: [
                    { id: "edit", icon: "edit", text: I18n.t("rename.action") },
                    { id: "copy", icon: "copy", text: I18n.t("detail.copy_path") },
                    { id: "delete", icon: "trash", text: I18n.t("delete.action"), guarded: true }
                  ]
                  onChosen: id => lab.lastAction = id
                }
                Label { anchors.verticalCenter: parent.verticalCenter; text: lab.lastAction; color: Theme.dim; font.pixelSize: 11 }
              }
            }
            ControlLabRow {
              width: parent.width
              title: I18n.t("lab.chip_strip")
              ChipStrip {
                width: Math.min(320, lab.width - 260)
                Repeater {
                  model: [I18n.t("lab.option_first"), I18n.t("lab.option_second"), I18n.t("lab.option_third")]
                  delegate: Chip {
                    required property string modelData
                    text: modelData.toUpperCase()
                    checked: lab.stripChoice === modelData
                    dim: !checked
                    onClicked: lab.stripChoice = modelData
                  }
                }
                Repeater {
                  model: [I18n.t("lab.sample_tag"), I18n.t("lab.sample_tag_second")]
                  delegate: TagChip {
                    required property string modelData
                    tag: modelData
                    checked: lab.pickedTag === modelData
                    dim: !checked
                    onClicked: lab.pickedTag = checked ? "" : modelData
                    onRenameRequested: lab.lastAction = "rename " + modelData
                    onDeleteRequested: lab.lastAction = "delete " + modelData
                  }
                }
              }
            }
            ControlLabRow {
              width: parent.width
              title: I18n.t("lab.color_picker")
              ColorPicker {
                chosen: lab.chosenColor
                automatic: Theme.primary
                onPicked: color => lab.chosenColor = color
              }
            }
            ControlLabRow {
              width: parent.width
              title: I18n.t("lab.context_chips")
              ContextChips {
                on: lab.contextSlots
                onChanged: slots => lab.contextSlots = slots
              }
            }
          }

          Column {
            width: parent.width
            spacing: 14
            visible: lab.section === "inputs"

            ControlLabRow {
              width: parent.width
              title: I18n.t("lab.field")
              Column {
                spacing: 14
                Field {
                  width: Math.min(420, lab.width - 260)
                  label: I18n.t("lab.sample_label")
                  value: lab.fieldValue
                  placeholder: I18n.t("lab.sample_placeholder")
                  onEdited: text => lab.fieldValue = text
                }
                Field {
                  width: Math.min(420, lab.width - 260)
                  label: I18n.t("lab.sample_label")
                  value: lab.fieldValue
                  placeholder: I18n.t("lab.sample_placeholder")
                  error: lab.fieldValue.trim() ? "" : I18n.t("settings.error_empty")
                  onEdited: text => lab.fieldValue = text
                }
              }
            }
            ControlLabRow {
              width: parent.width
              title: I18n.t("lab.number_field")
              NumberField {
                width: Math.min(420, parent.width)
                label: I18n.t("lab.sample_label")
                number: lab.fieldNumber
                onNumberEdited: number => lab.fieldNumber = number
              }
            }
            ControlLabRow {
              width: parent.width
              title: I18n.t("lab.text_input")
              Column {
                spacing: 8
                TextBox { width: Math.min(420, lab.width - 260); placeholderText: I18n.t("lab.sample_placeholder") }
                TextBox { width: Math.min(420, lab.width - 260); dense: true; placeholderText: I18n.t("lab.dense_placeholder") }
              }
            }
            ControlLabRow {
              width: parent.width
              title: I18n.t("lab.tag_input")
              TagInput {
                width: Math.min(420, lab.width - 260)
                tags: lab.labTags
                known: [I18n.t("lab.sample_tag"), I18n.t("lab.sample_tag_second")]
                onAdded: tag => lab.labTags = lab.labTags.concat([tag])
                onRemoved: tag => lab.labTags = lab.labTags.filter(kept => kept !== tag)
              }
            }
            ControlLabRow {
              width: parent.width
              title: I18n.t("lab.settings_list")
              Column {
                width: Math.min(420, lab.width - 260)
                spacing: 18
                SettingsList {
                  width: parent.width
                  items: lab.listItems
                  label: I18n.t("lab.list_typed")
                  placeholder: I18n.t("lab.sample_placeholder")
                  browse: true
                  nameFilters: Eco.textFileFilters()
                  onChanged: items => lab.listItems = items
                }
              }
            }
            ControlLabRow {
              width: parent.width
              title: I18n.t("lab.settings_card")
              ColumnLayout {
                width: Math.min(420, lab.width - 260)
                SettingsCard {
                  id: labCard
                  title: lab.fieldValue
                  placeholder: I18n.t("lab.sample_placeholder")
                  note: I18n.t("lab.sample_label")
                  removeTip: I18n.t("settings.remove")
                  movable: true
                  first: true
                  onToggled: expanded = !expanded
                  onRemoved: lab.fieldValue = ""
                  Field { Layout.fillWidth: true; label: I18n.t("lab.sample_label"); value: lab.fieldValue; onEdited: text => lab.fieldValue = text }
                }
                SettingsCard {
                  title: I18n.t("lab.sample_kept")
                  note: I18n.t("lab.sample_label")
                  removable: false
                  movable: true
                  last: true
                  onToggled: expanded = !expanded
                  Field { Layout.fillWidth: true; label: I18n.t("lab.sample_label"); value: lab.fieldValue; onEdited: text => lab.fieldValue = text }
                }
              }
            }
            ControlLabRow {
              width: parent.width
              title: I18n.t("lab.text_area")
              SettingsTextArea {
                width: Math.min(420, parent.width)
                label: I18n.t("lab.sample_label")
                value: lab.areaValue
                onEdited: text => lab.areaValue = text
              }
            }
            ControlLabRow {
              width: parent.width
              title: I18n.t("lab.multiline_input")
              TextBlock {
                width: Math.min(420, parent.width)
                placeholderText: I18n.t("lab.sample_placeholder")
              }
            }
            Kicker { width: parent.width; text: I18n.t("lab.chat_composer") }
            LabComposer { width: parent.width }
          }

          Column {
            width: parent.width
            spacing: 14
            visible: lab.section === "feedback"
            LabFeedbackSamples { width: parent.width }
          }

          Column {
            width: parent.width
            spacing: 14
            visible: lab.section === "data"
            Kicker { width: parent.width; text: I18n.t("lab.data_table") }
            LabSessionsTable { width: parent.width }
            LabDataSamples { width: parent.width }
          }

          Column {
            width: parent.width
            spacing: 14
            visible: lab.section === "surfaces"
            LabSurfaceSamples { width: parent.width }
          }

          Column {
            width: parent.width
            spacing: 14
            visible: lab.section === "motion"
            LabMotionSamples { width: parent.width }
          }

          Column {
            width: parent.width
            spacing: 14
            visible: lab.section === "icons"
            Kicker { width: parent.width; text: I18n.t("lab.icon_sets") }
            Label {
              width: parent.width
              text: I18n.t("lab.icon_sets_help")
              color: Theme.dim
              font.pixelSize: 11
              wrapMode: Text.WordWrap
            }
            LabIconGallery { width: parent.width }
          }
        }
      }
    }
  }

  ShortcutsDialog { id: labShortcuts }
}
