import QtQuick
import QtTest
import Eco.Core
import Eco.Settings

// An item opened in the Secrets, Providers, Models and Context tabs, run by
// tests/window.rs: an entry eco ships, held in the config like any other, and
// the global context open with BACK in the command bar, Esc and BACK go back
// to the table with the row still picked, and EDIT and DELETE are on for an
// entry nothing uses. Each check that fails is printed, and the exit status
// is how many failed.
QtObject {
  id: root

  property int failed: 0
  property ConfigWindow window: ConfigWindow {}
  property TestCase keys: null

  function check(name, ok) {
    if (!ok) {
      console.error("FAIL: " + name)
      failed += 1
    }
  }

  // Opens `id` in the section `name`.
  function open(name, id) {
    root.window.choose(root.window.view.at[name])
    root.window.view.open(root.window.view.at[name], id)
  }
  // The item open in section `name` is the entry's own view with BACK in the
  // bar; then `leave` goes back.
  function shows(name, id, leave) {
    const view = root.window.view
    const page = view.openPage
    root.check(name + " " + id + " opens", page !== null && page.key === id)
    root.check(name + " " + id + " has BACK in the command bar", view.commands.some(command => command.role === "back" && command.shown !== false))
    leave()
  }
  function returns(name, id, how) {
    const view = root.window.view
    root.check(how + " goes back to the " + name + " table", view.openPage === null)
    root.check(how + " leaves " + name + " " + id + " picked", view.picked[view.at[name]] === id)
  }
  // EDIT and DELETE of the picked row `id` in section `name` are on.
  function editable(name, id) {
    const view = root.window.view
    view.picked = view.setOf(view.picked, view.at[name], id)
    const bar = view.tableCommands(view.at[name])
    root.check(name + " " + id + " has EDIT on", bar.find(command => command.text === I18n.t("settings.edit")).enabled === true)
    root.check(name + " " + id + " has DELETE on", bar.find(command => command.text === I18n.t("settings.delete")).enabled === true)
  }
  function esc() { root.keys.keyClick(Qt.Key_Escape) }
  function back() { root.window.view.openPage.back() }

  readonly property var steps: [
    () => {
      Eco.handle({ type: "config", devices: [], omapass: { installed: true, page: "" }, credential_manager: false, platform: "linux",
        config: { rules: "", participants: [], kinds: ["meeting"], actions: [], contexts: [], context: { files: [] }, ui: { language: "auto" },
          secrets: [{ name: "OPENROUTER", env: "OPENROUTER_API_KEY" }, { name: "DEEPGRAM", env: "DEEPGRAM_API_KEY" }],
          providers: [{ name: "OPENROUTER", base_url: "https://openrouter.ai/api/v1", secret: "OPENROUTER" },
            { name: "DEEPGRAM", base_url: "wss://api.deepgram.com/v1/listen" }],
          models: [{ name: "nova-3", type: "transcription", provider: "OPENROUTER", model: "nova-3" }],
          stt: { model: "", language: "auto", languages: ["auto"] }, llm: { model: "", max_context_chars: 60000, concurrency: 8 } } })
      root.keys = Qt.createQmlObject("import QtTest; TestCase { when: false }", root.window.contentItem, "keys")
      Eco.configOpen = true
    },
    () => { root.editable("secrets", "DEEPGRAM"); root.open("secrets", "DEEPGRAM") },
    () => root.shows("secrets", "DEEPGRAM", root.esc),
    () => { root.returns("secrets", "DEEPGRAM", "Esc"); root.open("secrets", "DEEPGRAM") },
    () => root.shows("secrets", "DEEPGRAM", root.back),
    () => { root.returns("secrets", "DEEPGRAM", "BACK"); root.editable("providers", "DEEPGRAM"); root.open("providers", "DEEPGRAM") },
    () => root.shows("providers", "DEEPGRAM", root.esc),
    () => { root.returns("providers", "DEEPGRAM", "Esc"); root.open("providers", "DEEPGRAM") },
    () => root.shows("providers", "DEEPGRAM", root.back),
    () => { root.returns("providers", "DEEPGRAM", "BACK"); root.editable("models", "nova-3"); root.open("models", "nova-3") },
    () => root.shows("models", "nova-3", root.esc),
    () => { root.returns("models", "nova-3", "Esc"); root.open("models", "nova-3") },
    () => root.shows("models", "nova-3", root.back),
    () => { root.returns("models", "nova-3", "BACK"); root.open("context", "context") },
    () => root.shows("context", "context", root.back),
    () => { root.returns("context", "context", "BACK"); Qt.exit(root.failed) }
  ]

  property Timer step: Timer {
    property int at: 0
    interval: 400
    repeat: true
    running: true
    onTriggered: root.steps[at++]()
  }
}
