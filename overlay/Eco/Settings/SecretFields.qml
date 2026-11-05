pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Layouts
import Eco.Core
import Eco.Kit

// SecretFields edit where one key comes from, never the key: the source — an
// environment variable, an omapass password or, on Windows, a Credential
// Manager name — and the reference, typed or picked from the names the daemon
// lists. Beside a variable they say how to set it so the daemon sees it on the
// system it runs on. `secret` is the entry being edited; `chosen` says the
// source and the reference it now has.
ColumnLayout {
  id: frame

  required property var secret
  // What is wrong with the reference.
  property string error: ""
  readonly property string source: Draft.sourceOf(secret)
  readonly property string reference: secret[source] || ""
  // The sources this machine offers, in the order the picker lists them.
  readonly property var sources: ["env"].concat(Eco.credentialManager ? ["credential"] : Eco.omapassInstalled ? ["omapass"] : [])

  signal chosen(string source, string name)

  // How to set the variable `name` so the daemon sees it, on the system it runs on.
  function setHint(name) {
    return I18n.t(Eco.setHintKey, { name: name || "NAME" })
  }
  function title(source) { return I18n.t(Draft.sourceKey(source)) }
  // An option's two parts: {head: "<source>" or "type", name}.
  function parts(option) {
    const at = option.indexOf(":")
    return { head: option.slice(0, at), name: option.slice(at + 1) }
  }

  spacing: 18

  ColumnLayout {
    Layout.fillWidth: true
    spacing: 6
    Caption { text: I18n.t("secrets.column_source") }
    Flow {
      Layout.fillWidth: true
      spacing: 6
      Repeater {
        model: frame.sources
        delegate: Chip {
          required property string modelData
          text: frame.title(modelData)
          checked: frame.source === modelData
          onClicked: if (!checked) frame.chosen(modelData, "")
        }
      }
    }
  }

  ColumnLayout {
    Layout.fillWidth: true
    spacing: 6
    Caption { text: I18n.t("secrets.column_reference") }
    RowLayout {
      Layout.fillWidth: true
      spacing: 6
      SyncedBox {
        id: referenceBox
        Layout.fillWidth: true
        dense: true
        invalid: frame.error !== ""
        value: frame.reference
        placeholderTextColor: Theme.placeholder
        placeholderText: I18n.t(frame.source === "credential" ? "provider.credential_placeholder"
          : frame.source === "omapass" ? "provider.omapass_placeholder" : "provider.key_placeholder")
        onTextEdited: frame.chosen(frame.source, text)
      }
      // Each source's names, under its title, then a way to type one by hand.
      // An option is "<source>:<name>", or "type:<source>".
      Dropdown {
        label: I18n.t("provider.pick_key")
        current: frame.source + ":" + frame.reference
        options: frame.sources.reduce((options, source) => {
          const names = source === "env" ? Eco.keyNames
            : source === "credential" ? Eco.credentialNames || [] : Eco.omapassAccounts.map(account => account.account)
          return options.concat(names.map(name => source + ":" + name), ["type:" + source])
        }, [])
        section: option => {
          const part = frame.parts(option)
          return frame.title(part.head === "type" ? part.name : part.head)
        }
        describe: option => {
          const part = frame.parts(option)
          if (part.head === "type")
            return I18n.t("provider.key_type")
          const account = part.head === "omapass" ? Eco.omapassAccounts.find(a => a.account === part.name) : null
          return account && account.folder ? account.folder + " · " + part.name : part.name
        }
        onOpening: {
          Eco.requestKeys()
          if (frame.sources.includes("omapass"))
            Eco.requestOmapass()
        }
        onPicked: option => {
          const part = frame.parts(option)
          if (part.head !== "type")
            return frame.chosen(part.head, part.name)
          frame.chosen(part.name, part.name === frame.source ? frame.reference : "")
          referenceBox.forceActiveFocus()
        }
      }
      IconButton {
        visible: !Eco.credentialManager && !Eco.omapassInstalled && Eco.omapassPage !== ""
        name: "info"
        tip: I18n.t("provider.omapass_install")
        onClicked: Qt.openUrlExternally(Eco.omapassPage)
      }
    }
    FieldError { Layout.fillWidth: true; text: frame.error }
    ItemNote { visible: frame.source === "env"; text: frame.setHint(frame.reference) }
    ItemNote { text: I18n.t(Eco.credentialManager ? "secrets.alternative_credential" : "secrets.alternative_omapass") }
    ItemNote { text: I18n.t("provider.keys_env_hint") + (Eco.credentialManager ? " " + I18n.t("provider.keys_credential_hint") : "") }
    Label {
      Layout.fillWidth: true
      visible: Eco.credentialManager && Eco.credentialError !== ""
      text: I18n.t("provider.credentials_unlisted", { detail: Eco.credentialError.split("\n")[0] })
      color: Theme.error
      font.pixelSize: 10
      wrapMode: Text.Wrap
    }
    ItemNote { visible: frame.source === "credential" && frame.reference !== ""; text: I18n.t("provider.credential_how", { name: frame.reference }) }
    Label {
      Layout.fillWidth: true
      visible: frame.source === "omapass" && (!Eco.omapassInstalled || Eco.omapassError !== "" || Eco.omapassAccounts.length === 0)
      text: !Eco.omapassInstalled ? I18n.t("provider.omapass_missing")
          : Eco.omapassError !== "" ? Eco.omapassError.split("\n")[0] : I18n.t("provider.omapass_empty")
      color: !Eco.omapassInstalled || Eco.omapassError !== "" ? Theme.error : Theme.dim
      font.pixelSize: 10
      wrapMode: Text.Wrap
    }
  }
}
