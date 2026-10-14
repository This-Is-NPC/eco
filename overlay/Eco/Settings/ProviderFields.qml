pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Layouts
import Eco.Core
import Eco.Kit

// ProviderFields edit one provider (`stt`, or an `llm` model) — an
// OpenAI-compatible endpoint, or for `stt` a streaming one by its wss:// URL — in
// three steps: which provider (a preset, or a custom base URL), which model
// (typed, or picked from the list the provider offers, which LIST opens and
// closes), and where its key comes from (an environment variable, an omapass
// password, or on Windows a credential of the Credential Manager), typed or
// picked from the names the daemon lists; this window names keys and never
// reads one.
ColumnLayout {
  id: frame

  required property string target
  required property var section
  property var presets: []
  // What is wrong with the base URL and the model id, by key ("base_url", "model").
  property var errors: ({})
  // The custom endpoint was chosen; a base URL no preset has is custom too.
  property bool customChosen: false
  readonly property bool custom: customChosen || !presets.some(preset => preset.values.base_url === section.base_url)

  signal edited(string key, var value)

  // Gives the keyboard to the field of `key`, "base_url" or "model".
  function focusField(key) {
    if (key === "base_url")
      frame.customChosen = true
    ;(key === "base_url" ? baseUrl : modelId).forceActiveFocus()
  }

  // The omapass passwords are listed once a key taken from there is shown.
  onSectionChanged: {
    const fromOmapass = section && section.api_key_omapass !== undefined
    if (fromOmapass && Eco.omapassAccounts.length === 0 && Eco.omapassError === "")
      Eco.requestOmapass()
  }

  spacing: 18

  ColumnLayout {
    Layout.fillWidth: true
    spacing: 6
    Caption { text: I18n.t("provider.provider") }
    Flow {
      Layout.fillWidth: true
      spacing: 6
      Repeater {
        model: frame.presets
        delegate: Chip {
          required property var modelData
          text: modelData.name
          checked: !frame.custom && frame.section.base_url === modelData.values.base_url
          onClicked: {
            frame.customChosen = false
            // A key taken from omapass or a credential stays there; otherwise the preset names its variable.
            const kept = frame.section.api_key_omapass !== undefined || frame.section.api_key_credential !== undefined
            for (const key in modelData.values)
              if (!(kept && key === "api_key_env"))
                frame.edited(key, modelData.values[key])
            Eco.clearModels(frame.target)
          }
        }
      }
      Chip {
        text: I18n.t("provider.custom")
        checked: frame.custom
        onClicked: frame.customChosen = true
      }
    }
    SyncedBox {
      id: baseUrl
      Layout.fillWidth: true
      visible: frame.custom
      dense: true
      invalid: !!frame.errors.base_url
      value: frame.section.base_url || ""
      placeholderTextColor: Theme.placeholder
      placeholderText: I18n.t("provider.base_url")
      onTextEdited: {
        frame.edited("base_url", text)
        Eco.clearModels(frame.target)
      }
    }
    FieldError { Layout.fillWidth: true; text: frame.errors.base_url || "" }
  }

  ColumnLayout {
    Layout.fillWidth: true
    spacing: 6
    Caption { text: I18n.t("provider.model").toUpperCase() }
    RowLayout {
      Layout.fillWidth: true
      spacing: 6
      SyncedBox {
        id: modelId
        Layout.fillWidth: true
        dense: true
        invalid: !!frame.errors.model
        value: frame.section.model || ""
        // The model of the chosen preset, or of the first, as an example.
        placeholderTextColor: Theme.placeholder
        placeholderText: {
          const preset = frame.presets.find(p => p.values.base_url === frame.section.base_url) || frame.presets[0]
          return preset ? I18n.t("provider.model_placeholder", { model: preset.values.model }) : ""
        }
        onTextEdited: frame.edited("model", text)
      }
      TraceButton {
        // Open: the list is shown, or its error or emptiness is; pressing again closes it.
        readonly property bool open: Eco.modelsAsked[frame.target] && !Eco.modelsLoading[frame.target]
        dense: true
        role: "progress"
        icon: open ? "close" : "list"
        text: I18n.t(open ? "provider.list_close" : "provider.list")
        tip: I18n.t(open ? "provider.list_close_tip" : "provider.list_tip")
        busy: Eco.modelsLoading[frame.target]
        onClicked: open ? Eco.clearModels(frame.target) : Eco.requestModels(frame.target, frame.section)
      }
    }
    FieldError { Layout.fillWidth: true; text: frame.errors.model || "" }
    Label {
      Layout.fillWidth: true
      visible: Eco.modelsError[frame.target].length > 0
      text: I18n.t("provider.no_models", { detail: Eco.modelsError[frame.target].split("\n")[0] })
      color: Theme.error
      font.pixelSize: 10
      elide: Text.ElideRight
    }
    Label {
      Layout.fillWidth: true
      visible: Eco.modelsAsked[frame.target] && !Eco.modelsLoading[frame.target] && Eco.models[frame.target].length === 0 && Eco.modelsError[frame.target].length === 0
      text: I18n.t("settings.models_empty")
      color: Theme.dim
      font.pixelSize: 10
    }
    // The models the provider offers, once listed: filter and pick one, with
    // ↑/↓ and Enter from the filter.
    SurfaceFrame {
      Layout.fillWidth: true
      visible: Eco.models[frame.target].length > 0
      implicitHeight: 170
      ColumnLayout {
        anchors { fill: parent; margins: 6 }
        spacing: 4
        TextBox {
          id: filter
          Layout.fillWidth: true
          dense: true
          placeholderTextColor: Theme.placeholder
          placeholderText: I18n.t("provider.filter", { n: Eco.models[frame.target].length })
          onTextChanged: list.currentIndex = 0
          Keys.onDownPressed: list.incrementCurrentIndex()
          Keys.onUpPressed: list.decrementCurrentIndex()
          onAccepted: if (list.currentItem) list.currentItem.picked()
          Keys.onEscapePressed: Eco.clearModels(frame.target)
        }
        ListView {
          id: list
          Layout.fillWidth: true
          Layout.fillHeight: true
          clip: true
          // A row's outline never lies on the clip's edge, where a fractional
          // scale would cut it.
          topMargin: 1
          bottomMargin: 1
          model: Eco.models[frame.target].filter(id => id.toLowerCase().includes(filter.text.toLowerCase()))
          delegate: ChoiceRow {
            required property string modelData
            required property int index
            width: list.width
            text: modelData
            chosen: modelData === frame.section.model
            current: filter.activeFocus && index === list.currentIndex
            elide: Text.ElideMiddle
            onPicked: frame.edited("model", modelData)
          }
        }
      }
    }
  }

  ColumnLayout {
    id: key
    Layout.fillWidth: true
    spacing: 6
    // "credential" while the key is a Credential Manager credential, "omapass"
    // while it is an omapass password; a variable otherwise.
    readonly property string source: frame.section.api_key_credential !== undefined ? "credential"
      : frame.section.api_key_omapass !== undefined ? "omapass" : "env"
    readonly property string name: frame.section["api_key_" + source] || ""
    // The sources this machine offers, in the order the picker lists them.
    readonly property var sources: ["env"].concat(Eco.credentialManager ? ["credential"] : Eco.omapassInstalled ? ["omapass"] : [])

    // Keeps the key in the field of `source`, under `name`, and clears the others.
    function choose(source, name) {
      for (const other of ["env", "credential", "omapass"])
        if (other !== source)
          frame.edited("api_key_" + other, null)
      frame.edited("api_key_" + source, name)
    }
    // An option's two parts: {head: "<source>" or "type", name}.
    function parts(option) {
      const at = option.indexOf(":")
      return { head: option.slice(0, at), name: option.slice(at + 1) }
    }
    function title(source) {
      return I18n.t(source === "credential" ? "provider.key_credential" : source === "omapass" ? "provider.key_omapass" : "provider.key_env")
    }

    Caption { text: I18n.t("provider.key_from_source", { source: key.title(key.source) }) }
    RowLayout {
      Layout.fillWidth: true
      spacing: 6
      SyncedBox {
        id: keyName
        Layout.fillWidth: true
        dense: true
        value: key.name
        placeholderTextColor: Theme.placeholder
        placeholderText: I18n.t(key.source === "credential" ? "provider.credential_placeholder"
          : key.source === "omapass" ? "provider.omapass_placeholder" : "provider.key_placeholder")
        onTextEdited: frame.edited("api_key_" + key.source, text)
      }
      // Each source's names, under its title, then a way to type one by hand.
      // An option is "<source>:<name>", or "type:<source>".
      Dropdown {
        label: I18n.t("provider.pick_key")
        current: key.source + ":" + key.name
        options: key.sources.reduce((options, source) => {
          const names = source === "env" ? Eco.keyNames
            : source === "credential" ? Eco.credentialNames || [] : Eco.omapassAccounts.map(account => account.account)
          return options.concat(names.map(name => source + ":" + name), ["type:" + source])
        }, [])
        section: option => {
          const part = key.parts(option)
          return key.title(part.head === "type" ? part.name : part.head)
        }
        describe: option => {
          const part = key.parts(option)
          if (part.head === "type")
            return I18n.t("provider.key_type")
          const account = part.head === "omapass" ? Eco.omapassAccounts.find(a => a.account === part.name) : null
          return account && account.folder ? account.folder + " · " + part.name : part.name
        }
        onOpening: {
          Eco.requestKeys()
          if (key.sources.includes("omapass"))
            Eco.requestOmapass()
        }
        onPicked: option => {
          const part = key.parts(option)
          if (part.head !== "type")
            return key.choose(part.head, part.name)
          key.choose(part.name, part.name === key.source ? key.name : "")
          keyName.forceActiveFocus()
        }
      }
      IconButton {
        visible: !Eco.credentialManager && !Eco.omapassInstalled && Eco.omapassPage !== ""
        name: "info"
        tip: I18n.t("provider.omapass_install")
        onClicked: Qt.openUrlExternally(Eco.omapassPage)
      }
    }
    Label {
      Layout.fillWidth: true
      text: I18n.t("provider.keys_env_hint") + (Eco.credentialManager ? " " + I18n.t("provider.keys_credential_hint") : "")
      color: Theme.dim
      font.pixelSize: 10
      wrapMode: Text.Wrap
    }
    Label {
      Layout.fillWidth: true
      visible: Eco.credentialManager && Eco.credentialError !== ""
      text: I18n.t("provider.credentials_unlisted", { detail: Eco.credentialError.split("\n")[0] })
      color: Theme.error
      font.pixelSize: 10
      wrapMode: Text.Wrap
    }
    Label {
      Layout.fillWidth: true
      visible: key.source === "credential" && key.name !== ""
      text: I18n.t("provider.credential_how", { name: key.name })
      color: Theme.dim
      font.pixelSize: 10
      wrapMode: Text.Wrap
    }
    Label {
      Layout.fillWidth: true
      visible: key.source === "omapass" && (!Eco.omapassInstalled || Eco.omapassError !== "" || Eco.omapassAccounts.length === 0)
      text: !Eco.omapassInstalled ? I18n.t("provider.omapass_missing")
          : Eco.omapassError !== "" ? Eco.omapassError.split("\n")[0] : I18n.t("provider.omapass_empty")
      color: !Eco.omapassInstalled || Eco.omapassError !== "" ? Theme.error : Theme.dim
      font.pixelSize: 10
      wrapMode: Text.Wrap
    }
  }

  Item { Layout.fillHeight: true }
}
