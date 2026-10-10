pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Layouts
import Eco.Core
import Eco.Kit

// StartDialog opens a session: an optional title, its kind, the language it is
// held in, optional tags and the context slots it sends (those of its kind to begin with); it
// says which models its kind runs on and lists who will be heard, so a wrong
// model or device shows before starting.
ModalDialog {
  id: dialog

  property string language: Eco.language
  property string kind: Eco.kinds.length > 0 ? Eco.kinds[0] : ""
  property var slots: []
  property var tags: []
  // The kind and language the last session started here used, while this
  // window lives: the next one starts from them.
  property string lastKind: ""
  property string lastLanguage: ""
  // When the dialog opened, for the title it suggests.
  property date openedAt: new Date()
  // The title a session gets when none is typed: its kind and the time.
  readonly property string suggestedTitle: I18n.t("dialog.suggested_title", { kind: Eco.kindName(kind), time: openedAt.toLocaleTimeString(I18n.locale, Locale.ShortFormat) })
  readonly property bool canStart: Eco.inputs.length > 0
  onKindChanged: slots = Eco.contextsOfKind(kind)

  height: Math.min(parent.height - 48, panel.implicitHeight)

  onOpened: {
    titleBox.text = ""
    openedAt = new Date()
    language = Eco.languages.includes(lastLanguage) ? lastLanguage : Eco.language
    kind = Eco.kinds.includes(lastKind) ? lastKind : (Eco.kinds.length > 0 ? Eco.kinds[0] : "")
    slots = Eco.contextsOfKind(kind)
    tags = []
    titleBox.forceActiveFocus()
  }

  function start() {
    if (!canStart)
      return
    // A tag typed and not yet added goes with it.
    tagsBox.add(tagsBox.typed)
    lastKind = kind
    lastLanguage = language
    Eco.startSession(titleBox.text.trim() || suggestedTitle, kind, language, slots, tags)
    close()
  }

  contentItem: DialogPanel {
    id: panel
    dialogOpen: dialog.visible
    implicitHeight: frame.implicitHeight + 56
    title: I18n.t("dialog.title")
    active: true

    // The form scrolls in a short window; its buttons stay in reach below it.
    ColumnLayout {
      id: frame
      anchors.fill: parent
      spacing: 18

      ScrollPage {
        id: scroll
        Layout.fillWidth: true
        Layout.fillHeight: true
        Layout.preferredHeight: form.implicitHeight
        contentHeight: form.implicitHeight

        ColumnLayout {
          id: form
          width: scroll.width
          spacing: 18

          ColumnLayout {
            Layout.fillWidth: true
            spacing: 2
            Caption { text: I18n.t("dialog.name") }
            TextBox {
              id: titleBox
              Layout.fillWidth: true
              placeholderText: dialog.suggestedTitle
              onAccepted: dialog.start()
            }
          }

          Flow {
            Layout.fillWidth: true
            spacing: 24
            ColumnLayout {
              spacing: 4
              Caption { text: I18n.t("dialog.kind") }
              Dropdown {
                options: Eco.kinds
                current: dialog.kind
                describe: kind => Eco.kindName(kind)
                onPicked: kind => dialog.kind = kind
              }
            }
            ColumnLayout {
              spacing: 4
              Caption { text: I18n.t("dialog.language") }
              Dropdown {
                options: Eco.languages
                current: dialog.language
                describe: code => Eco.languageName(code)
                onPicked: code => dialog.language = code
              }
            }
          }

          Label {
            Layout.fillWidth: true
            visible: !!Eco.kindModels[dialog.kind]
            text: Eco.kindModels[dialog.kind] ? I18n.t("dialog.kind_models", Eco.kindModels[dialog.kind]) : ""
            color: Theme.dim
            font.pixelSize: 11
            wrapMode: Text.Wrap
          }

          ColumnLayout {
            Layout.fillWidth: true
            spacing: 4
            Caption { text: I18n.t("tags.short") }
            TagInput {
              id: tagsBox
              Layout.fillWidth: true
              tags: dialog.tags
              known: Eco.tagNames
              onAdded: tag => dialog.tags = dialog.tags.concat([tag])
              onRemoved: tag => dialog.tags = dialog.tags.filter(kept => kept !== tag)
            }
          }

          ColumnLayout {
            Layout.fillWidth: true
            visible: Eco.contexts.length > 0
            spacing: 4
            Caption { text: I18n.t("contexts.short") }
            ContextChips {
              Layout.fillWidth: true
              on: dialog.slots
              onChanged: slots => dialog.slots = slots
            }
          }

          ColumnLayout {
            Layout.fillWidth: true
            spacing: 4
            Caption { text: I18n.t("dialog.heard") }
            Repeater {
              model: Eco.inputs
              delegate: Label {
                required property var modelData
                Layout.fillWidth: true
                text: modelData.participant + (Eco.isUser(modelData.participant) ? " " + I18n.t("dialog.you") : "") + "  ·  " + Eco.deviceLabel(modelData.id, modelData.label)
                color: Eco.identityColor(modelData.participant, "")
                font.pixelSize: 11
                elide: Text.ElideRight
              }
            }
            Label {
              Layout.fillWidth: true
              visible: !dialog.canStart
              text: I18n.t("dialog.no_inputs")
              color: Theme.warning
              font.pixelSize: 11
              wrapMode: Text.Wrap
            }
            Chip {
              visible: !dialog.canStart
              icon: "settings"
              text: I18n.t("dialog.audio_settings")
              onClicked: { dialog.close(); Eco.openConfigAt(0) }
            }
          }
        }
      }

      DialogFooter {
        primaryIcon: "enter"
        primaryText: I18n.t("dialog.start")
        primaryEnabled: dialog.canStart
        onCancelled: dialog.close()
        onAccepted: dialog.start()
      }
    }
  }
}
