pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Dialogs
import QtQuick.Layouts
import Eco.Core
import Eco.Kit

// AddFileScreen adds text files to a context: the path typed, added with ADD
// or Enter, or the files chosen with BROWSE, added at once. Either saves.
ChildScreen {
  id: adding

  // The context the files go to: "" for the global one, else a slot's name.
  required property string slot
  property string path: ""
  readonly property string typed: path.trim()

  // Adds `paths` to the context and returns to its files, the first picked.
  function add(paths) {
    if (paths.length > 0 && !Eco.configSaving)
      finish({ change: d => Draft.addFiles(Draft.contextFiles(d, adding.slot), paths), pick: paths[0] })
  }

  title: I18n.t("contexts.add_file")
  note: I18n.t("contexts.add_file_note")
  commands: [
    { text: I18n.t("contexts.add_file"), icon: "plus", role: "primary", tip: I18n.t("contexts.add_file_save_tip"),
      enabled: typed !== "" && !Eco.configSaving, run: () => adding.add([adding.typed]) },
    { text: I18n.t("import.browse"), icon: "folder", tip: I18n.t("settings.browse"), run: () => picker.open() }
  ]

  Field {
    id: field
    Layout.fillWidth: true
    label: I18n.t("contexts.file_path")
    value: adding.path
    placeholder: I18n.t("settings.file_placeholder")
    onEdited: text => adding.path = text
  }
  Connections {
    target: field.input
    function onAccepted() {
      if (adding.typed !== "")
        adding.add([adding.typed])
    }
  }
  FileDialog {
    id: picker
    title: I18n.t("settings.browse")
    fileMode: FileDialog.OpenFiles
    nameFilters: Eco.textFileFilters()
    onAccepted: adding.add(selectedFiles.map(url => Eco.localPath(url)))
  }
}
