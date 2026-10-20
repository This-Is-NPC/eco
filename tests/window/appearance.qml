import QtQuick
import QtTest
import Eco.Core
import Eco.Kit
import Eco.Settings

// The real theme and controls, driven with synthetic system and daemon state.
QtObject {
  id: root
  property int failed: 0
  property string sent: ""
  property var sentDraft: null
  property Item saver: Item {
    function saveNow(change) {
      const next = JSON.parse(JSON.stringify(Eco.config))
      change(next)
      root.sentDraft = next
    }
  }
  property AppearanceSection section: AppearanceSection { settings: root.saver }
  property ColorField field: ColorField { value: "#123456"; onPicked: color => root.sent = color }
  property SettingsView settings: SettingsView {}

  function check(name, ok) {
    if (!ok) {
      console.error("FAIL: " + name)
      failed++
    }
  }
  function config(appearance) {
    return { rules: "", participants: [], models: [], kinds: ["meeting"], actions: [], contexts: [], context: { files: [] },
      ui: { language: "auto", appearance: appearance },
      stt: { model: "", language: "auto", languages: ["auto"] }, llm: { model: "" } }
  }
  function saved(appearance) {
    Eco.handle({ type: "config", config: config(appearance), devices: [], presets: {}, omapass: { installed: false, page: "" }, credential_manager: false })
  }

  readonly property var steps: [
    () => {
      const system = { background: "#eeeeee", foreground: "#111111", accent: "#165027", mode: "light" }
      check("system follows the light preference without an Omarchy palette", Theme.resolve({ preset: "system" }, {}, false).background === Theme.light.background)
      check("system follows the dark preference without an Omarchy palette", Theme.resolve({ preset: "system" }, {}, true).background === Theme.dark.background)
      check("system prefers the Omarchy palette", Theme.resolve({ preset: "system" }, system, true).background === "#eeeeee")
      check("dark ignores both the system palette and light preference", Theme.resolve({ preset: "dark" }, system, false).background === Theme.dark.background)
      check("light ignores both the system palette and dark preference", Theme.resolve({ preset: "light" }, system, true).background === Theme.light.background)
      saved({ preset: "light", colors: {} })
    },
    () => {
      check("the saved light preset reaches the window", Theme.mode === "light" && String(Theme.background) === Theme.light.background)
      saved({ preset: "dark", colors: {} })
    },
    () => {
      check("the saved dark preset reaches the window", Theme.mode === "dark" && String(Theme.background) === Theme.dark.background)
      Theme.readSystem('mode = "light"\nbackground = "#eeeeee"\nforeground = "#111111"\naccent = "#165027"')
      Theme.configure({ preset: "system" })
    },
    () => {
      check("system applies the current Omarchy theme", String(Theme.background) === "#eeeeee" && Theme.mode === "light")
      Theme.readSystem('mode = "dark"\nbackground = "#181818"\nforeground = "#eeeeee"')
    },
    () => {
      check("system follows an Omarchy theme change", String(Theme.background) === "#181818" && Theme.mode === "dark")
      Theme.readSystem('background = "#ffffff"')
      Theme.readSystem("")
    },
    () => {
      check("a half-written theme keeps the last complete palette", String(Theme.background) === "#181818")
      saved({ preset: "custom", colors: { background: "#102030", foreground: "#eeeeee", accent: "#ffffff", muted: "#345678" } })
    },
    () => {
      check("custom applies saved colors", String(Theme.background) === "#102030" && String(Theme.primary) === "#ffffff")
      check("custom border colors reach the shared kit", String(Theme.border) === "#345678" && String(Theme.line) !== String(Theme.foreground))
      Theme.readSystem('background = "#ffffff"\nforeground = "#000000"')
    },
    () => {
      check("custom ignores system changes", String(Theme.background) === "#102030")
      Theme.configure({ preset: "custom", colors: { background: "#ffffff", foreground: "#ffffff" } })
    },
    () => {
      check("low contrast keeps displayed text readable", Theme.contrast(Theme.rgb(String(Theme.background)), Theme.rgb(String(Theme.foreground))) >= 4.5)
      field.commit("red")
      check("invalid colors are never sent", sent === "")
      field.commit(" #ABCDEF ")
      check("hexadecimal colors are normalized and sent", sent === "#abcdef")
      saved({ preset: "dark", colors: { accent: "#abcdef" } })
    },
    () => {
      check("appearance is the ninth settings tab", settings.at.appearance === 8 && settings.sections[8].name === I18n.t("settings.appearance"))
      check("all theme roles have controls", section.rows(Eco.config).length === 10)
      check("custom colors cannot be edited under dark", !section.custom)
      section.choose("custom")
      check("choosing custom preserves its saved colors", sentDraft.ui.appearance.preset === "custom" && sentDraft.ui.appearance.colors.accent === "#abcdef")
      saved({ preset: "light", colors: {} })
    },
    () => {
      section.choose("custom")
      check("the first custom palette starts with the current theme", sentDraft.ui.appearance.colors.background === Theme.light.background)
      const custom = sentDraft.ui.appearance
      saved(custom)
    },
    () => {
      const reset = section.commands(Eco.config, null)[0]
      reset.run()
      check("reset restores the custom defaults", JSON.stringify(sentDraft.ui.appearance.colors) === JSON.stringify(Theme.dark) && sentDraft.ui.appearance.preset === "custom")
      Qt.exit(failed)
    }
  ]
  property Timer timer: Timer {
    property int at: 0
    interval: 50
    running: true
    repeat: true
    onTriggered: root.steps[at++]()
  }
}
