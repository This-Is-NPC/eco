import QtQuick
import QtTest
import Eco.Core
import Eco.Settings

// An item opened in the Secrets, Providers, Models and Context tabs, run by
// tests/window.rs: a preset and the global context open with BACK in the
// command bar, Esc and BACK go back to the table with the row still picked,
// and a preset's facts are filled. Each check that fails is printed, and the
// exit status is how many failed.
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
  // The item open in section `name` has BACK in the bar and, if a preset, its
  // facts; then `leave` goes back.
  function shows(name, id, leave) {
    const view = root.window.view
    const page = view.openPage
    root.check(name + " " + id + " opens", page !== null && page.key === id)
    root.check(name + " " + id + " has BACK in the command bar", view.commands.some(command => command.role === "back" && command.shown !== false))
    if (page !== null && page.preset !== undefined)
      root.check(name + " " + id + " facts are filled", page.facts.length > 0 && page.facts[0][1] !== "")
    leave()
  }
  function returns(name, id, how) {
    const view = root.window.view
    root.check(how + " goes back to the " + name + " table", view.openPage === null)
    root.check(how + " leaves " + name + " " + id + " picked", view.picked[view.at[name]] === id)
  }
  function esc() { root.keys.keyClick(Qt.Key_Escape) }
  function back() { root.window.view.openPage.back() }

  readonly property var steps: [
    () => {
      Eco.handle({ type: "config", devices: [], omapass: { installed: true, page: "" }, credential_manager: false, platform: "linux",
        presets: {
          secrets: [{ name: "OPENROUTER", env: "OPENROUTER_API_KEY" }],
          providers: [{ name: "OPENROUTER", base_url: "https://openrouter.ai/api/v1", secret: "OPENROUTER" }],
          models: [{ name: "nova-3", type: "transcription", provider: "OPENROUTER", model: "nova-3" }]
        },
        config: { rules: "", participants: [], kinds: ["meeting"], actions: [], contexts: [], context: { files: [] }, ui: { language: "auto" },
          secrets: [], providers: [], models: [],
          stt: { model: "", language: "auto", languages: ["auto"] }, llm: { model: "", max_context_chars: 60000, concurrency: 8 } } })
      root.keys = Qt.createQmlObject("import QtTest; TestCase { when: false }", root.window.contentItem, "keys")
      Eco.configOpen = true
    },
    () => root.open("secrets", "OPENROUTER"),
    () => root.shows("secrets", "OPENROUTER", root.esc),
    () => { root.returns("secrets", "OPENROUTER", "Esc"); root.open("secrets", "OPENROUTER") },
    () => root.shows("secrets", "OPENROUTER", root.back),
    () => { root.returns("secrets", "OPENROUTER", "BACK"); root.open("providers", "OPENROUTER") },
    () => root.shows("providers", "OPENROUTER", root.esc),
    () => { root.returns("providers", "OPENROUTER", "Esc"); root.open("models", "nova-3") },
    () => root.shows("models", "nova-3", root.esc),
    () => { root.returns("models", "nova-3", "Esc"); root.open("context", "context") },
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
