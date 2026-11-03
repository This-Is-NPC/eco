import QtQuick
import QtTest
import Eco.Core
import Eco.Settings

// The TEST of a model, run by tests/window.rs: the button of a saved model is
// idle, busy while the daemon waits, and leaves its result among the model's
// facts, an answer with its time or an error in the interface language; the
// button of a preset that waits for a key is off, saying why. Each check that
// fails is printed, and the exit status is how many failed.
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

  // The first item under `item` that `is` holds for.
  function find(item, is) {
    for (const child of item.children) {
      if (is(child))
        return child
      const found = find(child, is)
      if (found)
        return found
    }
    return null
  }
  function models() { return window.view.sections[window.view.at.models] }
  // The TEST command, as the bar gets it, for the model `name`.
  function button(name) { return models().testCommand(name) }
  // What the model's page lists among its facts.
  function fact() { return models().testFacts("local") }

  readonly property var steps: [
    () => {
      Eco.handle({ type: "config", devices: [], omapass: { installed: true, page: "" }, credential_manager: false, platform: "linux",
        presets: {
          secrets: [{ name: "OPENROUTER", env: "OPENROUTER_API_KEY" }],
          providers: [{ name: "OPENROUTER", base_url: "https://openrouter.ai/api/v1", secret: "OPENROUTER" }],
          models: [{ name: "gpt-6-luna", type: "chat", provider: "OPENROUTER", model: "openai/gpt-6-luna", reasoning: "off" }]
        },
        config: { rules: "", participants: [], kinds: ["meeting"], actions: [], contexts: [], context: { files: [] }, ui: { language: "auto" },
          secrets: [{ name: "mine", env: "MINE_KEY" }],
          providers: [{ name: "lan", base_url: "http://lan:8000/v1", secret: "mine" }],
          models: [{ name: "local", type: "chat", provider: "lan", model: "qwen" }],
          stt: { model: "", language: "auto", languages: ["auto"] }, llm: { model: "local", max_context_chars: 60000, concurrency: 8 } } })
      Eco.disabledModels = { "gpt-6-luna": { code: "preset.key_missing", params: { name: "gpt-6-luna", variable: "OPENROUTER_API_KEY" } } }
      Eco.configOpen = true
    },
    () => {
      const idle = root.button("local")
      root.check("idle: TEST is on and not busy", idle.text === I18n.t("model_test.button") && idle.enabled === true && idle.busy === false)
      root.check("idle: no result is listed", root.fact().length === 0)
      const off = root.button("gpt-6-luna")
      root.check("a preset waiting for a key has TEST off, saying why", off.enabled === false && off.tip === root.models().disabledText("gpt-6-luna"))
      root.window.choose(root.window.view.at.models)
      root.window.view.open(root.window.view.at.models, "local", false)
      Eco.modelTests = { local: { busy: true } }
    },
    () => {
      const busy = root.button("local")
      root.check("busy: TEST says it is testing", busy.busy === true && busy.text === I18n.t("model_test.busy"))
      root.check("busy: the page hands the bar a busy TEST", root.window.view.commands.some(command => command.busy === true && command.text === I18n.t("model_test.busy")) !== null)
      const row = Qt.createQmlObject('import QtQuick\nimport Eco.Kit\nCommandRow { commands: [{ text: "T", busy: true, run: () => {} }] }', root.window)
      root.check("busy: the bar's button shows it", root.find(row, item => item.busy === true) !== null)
      root.check("busy: no result yet", root.fact().length === 0)
      Eco.handle({ type: "model_test", name: "local", ok: true, ms: 412 })
    },
    () => {
      const done = root.button("local")
      root.check("ok: the button is idle again", done.busy === false && done.enabled === true)
      root.check("ok: the time is listed", root.fact()[0][1] === I18n.t("model_test.ok", { ms: 412 }))
      Eco.handle({ type: "model_test", name: "local", ok: false, code: "model_test.credential", params: { name: "local", secret: "mine" },
        message: "local: refused" })
    },
    () => {
      const told = root.fact()[0][1]
      root.check("error: it is told in the interface language, naming the secret", told === I18n.t("error.model_test.credential", { name: "local", secret: "mine" }) && told.includes("mine"))
      root.check("error: the button is idle again", root.button("local").busy === false)
      Eco.handle({ type: "model_test", name: "local", ok: false, code: "model_test.timeout", params: { name: "local", seconds: 15 }, message: "" })
    },
    () => {
      root.check("a timeout is told", root.fact()[0][1] === I18n.t("error.model_test.timeout", { name: "local", seconds: 15 }))
      Eco.handle({ type: "config", devices: [], omapass: { installed: true, page: "" }, credential_manager: false, platform: "linux",
        presets: Eco.presets, config: Eco.config })
    },
    () => {
      root.check("a saved config drops the results", root.fact().length === 0)
      Qt.exit(root.failed)
    }
  ]

  property Timer step: Timer {
    property int at: 0
    interval: 400
    repeat: true
    running: true
    onTriggered: root.steps[at++]()
  }
}
