pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Dialogs
import QtQuick.Layouts

// ImportDialog turns an audio or video file, or a WebVTT transcript, into a
// session: the file, its title (the file's name when empty), when it was recorded
// (what the file says unless changed), kind, language and who is speaking in it
// (a transcript that names its speakers keeps them).
ModalDialog {
  id: dialog

  property string kind
  property string language
  property string participant
  // Whether starting was asked without a file: a hint says one is needed.
  property bool missing: false
  // When the file says it was recorded, as the date box writes it; "" until known.
  property string recorded: ""
  // The date in the box in seconds since the epoch, or null when it is not one.
  readonly property var date: {
    const parts = dateBox.text.trim().match(/^(\d{4})-(\d{2})-(\d{2})[ T](\d{2}):(\d{2})$/)
    if (!parts)
      return null
    const at = new Date(parts[1], parts[2] - 1, parts[3], parts[4], parts[5])
    return at.getMonth() === parts[2] - 1 && at.getDate() === Number(parts[3]) && at.getHours() === Number(parts[4]) ? at.getTime() / 1000 : null
  }
  readonly property bool badDate: dateBox.text.trim().length > 0 && date === null

  maxWidth: 560

  // Ask the daemon when the file in the box was recorded.
  function dateFile() {
    recorded = ""
    dateBox.text = ""
    if (pathBox.text.trim().length > 0)
      Eco.dateImport(pathBox.text.trim())
  }

  function choose(path) {
    pathBox.text = path
    dateFile()
    kind = Eco.kinds.length > 0 ? Eco.kinds[0] : ""
    language = Eco.language
    const user = Eco.participants.find(p => p.user)
    participant = user ? user.name : (Eco.participants.length > 0 ? Eco.participants[0].name : "")
    titleBox.text = ""
    missing = false
    open()
  }

  function start() {
    if (pathBox.text.trim().length === 0) {
      missing = true
      pathBox.forceActiveFocus()
      return
    }
    if (badDate) {
      dateBox.forceActiveFocus()
      return
    }
    const request = { path: pathBox.text.trim(), title: titleBox.text, kind: kind, language: language, participant: participant }
    // Left out, the daemon dates it by the file to the second.
    if (date !== null && dateBox.text.trim() !== recorded)
      request.started_at = date
    Eco.importFile(request)
    close()
  }

  onOpened: (pathBox.text ? titleBox : pathBox).forceActiveFocus()

  Connections {
    target: Eco
    function onImportDateChanged() {
      if (Eco.importDate.path !== pathBox.text.trim() || Eco.importDate.at === null)
        return
      dialog.recorded = Qt.formatDateTime(new Date(Eco.importDate.at * 1000), "yyyy-MM-dd hh:mm")
      dateBox.text = dialog.recorded
    }
  }

  FileDialog {
    id: picker
    title: I18n.t("import.title")
    nameFilters: [I18n.t("import.media") + " (*.mp4 *.mkv *.webm *.mov *.m4a *.mp3 *.ogg *.opus *.flac *.wav *.aac *.vtt)", I18n.t("import.any") + " (*)"]
    onAccepted: {
      pathBox.text = Eco.localPath(selectedFile)
      dialog.dateFile()
    }
  }

  contentItem: DialogPanel {
    dialogOpen: dialog.visible
    implicitHeight: form.implicitHeight + 56
    title: I18n.t("import.title")
    active: true

    ColumnLayout {
      id: form
      anchors.fill: parent
      spacing: 18

      ColumnLayout {
        Layout.fillWidth: true
        spacing: 2
        Caption { text: I18n.t("import.file") }
        RowLayout {
          Layout.fillWidth: true
          spacing: 8
          TextBox {
            id: pathBox
            Layout.fillWidth: true
            placeholderText: I18n.t("import.file_placeholder")
            onAccepted: dialog.start()
            onEditingFinished: if (Eco.importDate.path !== text.trim()) dialog.dateFile()
          }
          Chip { icon: "folder"; text: I18n.t("import.browse"); dim: true; onClicked: picker.open() }
        }
        Label {
          Layout.fillWidth: true
          Layout.topMargin: 4
          visible: dialog.missing && pathBox.text.trim().length === 0
          text: I18n.t("import.no_file")
          color: Theme.warning
          font.pixelSize: 11
          wrapMode: Text.Wrap
        }
      }

      ColumnLayout {
        Layout.fillWidth: true
        spacing: 2
        Caption { text: I18n.t("dialog.name") }
        TextBox {
          id: titleBox
          Layout.fillWidth: true
          placeholderText: I18n.t("import.name_placeholder")
          onAccepted: dialog.start()
        }
      }

      ColumnLayout {
        Layout.fillWidth: true
        spacing: 2
        Caption { text: I18n.t("import.date") }
        RowLayout {
          Layout.fillWidth: true
          spacing: 12
          TextBox {
            id: dateBox
            Layout.fillWidth: true
            placeholderText: I18n.t("import.date_placeholder")
            invalid: dialog.badDate
            onAccepted: dialog.start()
          }
          Label {
            visible: dialog.date !== null
            text: dialog.date !== null ? I18n.when(dialog.date) : ""
            color: Theme.dim
          }
        }
        FieldError {
          Layout.fillWidth: true
          Layout.topMargin: 4
          text: dialog.badDate ? I18n.t("import.bad_date") : ""
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
        ColumnLayout {
          visible: Eco.participants.length > 0
          spacing: 4
          Caption { text: I18n.t("import.speaker") }
          Dropdown {
            options: Eco.participants.map(p => p.name)
            current: dialog.participant
            onPicked: name => dialog.participant = name
          }
        }
      }

      DialogFooter {
        primaryIcon: "import"
        primaryText: I18n.t("import.start")
        primaryDim: pathBox.text.trim().length === 0
        onCancelled: dialog.close()
        onAccepted: dialog.start()
      }
    }
  }
}
