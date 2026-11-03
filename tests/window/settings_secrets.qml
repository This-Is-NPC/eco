import QtQuick
import QtTest
import Eco.Core
import Eco.Settings

// The Secrets, Providers and Models tabs, run by tests/window.rs: each table
// lists the user's entries and then the presets eco ships, a preset is read
// only and a model preset that waits for a key is listed disabled with the
// reason, whether a secret resolves is told without a key, a secret or provider
// in use is not deleted, and beside a variable the hint is the one of the
// system the daemon runs on. Each check that fails is printed, and the exit
// status is how many failed.
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
  // The texts of the rows' cells of the section `name`, one array per row.
  function table(name) {
    const view = root.window.view
    return view.sections[view.at[name]].rows(Eco.config).map(row => row.cells)
  }
  function row(name, id) {
    const view = root.window.view
    return view.sections[view.at[name]].rows(Eco.config).find(entry => entry.id === id)
  }

  readonly property var steps: [
    () => {
      Eco.handle({ type: "config", devices: [], omapass: { installed: true, page: "" }, credential_manager: false, platform: "linux",
        presets: {
          secrets: [{ name: "OPENROUTER", env: "OPENROUTER_API_KEY" }, { name: "DEEPGRAM", env: "DEEPGRAM_API_KEY" }],
          providers: [{ name: "OPENROUTER", base_url: "https://openrouter.ai/api/v1", secret: "OPENROUTER" },
            { name: "DEEPGRAM", base_url: "wss://api.deepgram.com/v1/listen", secret: "DEEPGRAM" }],
          models: [{ name: "gpt-6-luna", type: "chat", provider: "OPENROUTER", model: "openai/gpt-6-luna", reasoning: "off" },
            { name: "nova-3", type: "transcription", provider: "DEEPGRAM", model: "nova-3" }]
        },
        config: { rules: "", participants: [], kinds: ["meeting"], actions: [], contexts: [], context: { files: [] }, ui: { language: "auto" },
          secrets: [{ name: "mine", env: "MINE_KEY" }, { name: "DEEPGRAM", omapass: "deepgram" }],
          providers: [{ name: "lan", base_url: "http://lan:8000/v1", secret: "mine" }, { name: "free", base_url: "http://free/v1" }],
          models: [{ name: "local", type: "chat", provider: "lan", model: "qwen" }],
          stt: { model: "", language: "auto", languages: ["auto"] }, llm: { model: "local", max_context_chars: 60000, concurrency: 8 } } })
      Eco.handle({ type: "secrets", status: {
        mine: { resolved: true },
        DEEPGRAM: { resolved: false, code: "secret.omapass_failed", params: { account: "deepgram", detail: "omapass: locked" } },
        OPENROUTER: { resolved: false, code: "secret.env_unset", params: { variable: "OPENROUTER_API_KEY" } } } })
      Eco.disabledModels = { "gpt-6-luna": { code: "preset.key_missing", params: { name: "gpt-6-luna", variable: "OPENROUTER_API_KEY" } } }
      Eco.configOpen = true
    },
    () => {
      const secrets = root.table("secrets")
      root.check("Secrets lists the user's secrets, then the presets they do not replace", secrets.map(cells => cells[0]).join() === "mine,DEEPGRAM,OPENROUTER")
      root.check("a secret that resolves says so, without a key", secrets[0][3] === I18n.t("secrets.resolved") && secrets[0][2] === "MINE_KEY")
      root.check("a secret that does not says why", secrets[1][3] === I18n.t("error.secret.omapass_failed", { account: "deepgram", detail: "omapass: locked" }))
      root.check("a user secret replaces the preset of its name", secrets[1][1] === I18n.t("provider.key_omapass") && secrets[1][2] === "deepgram")
      root.check("a preset secret says why it does not resolve", secrets[2][3] === I18n.t("error.secret.env_unset", { variable: "OPENROUTER_API_KEY" }))
      root.check("a preset secret is read only", root.row("secrets", "OPENROUTER").fixed === true && root.row("secrets", "OPENROUTER").edits === false)
      root.check("a secret a provider uses is not deleted", root.row("secrets", "mine").removable === false)
      root.check("a secret nothing uses is", root.row("secrets", "DEEPGRAM").removable === true)
      const providers = root.table("providers")
      root.check("Providers lists the user's, then the presets", providers.map(cells => cells[0]).join() === "lan,free,OPENROUTER,DEEPGRAM")
      root.check("a provider shows its secret or none", providers[0][2] === "mine" && providers[1][2] === I18n.t("providers.no_secret"))
      root.check("a provider shows what uses it, the presets' models too", providers[0][3] === "local" && providers[2][3] === "gpt-6-luna")
      root.check("a provider a model uses is not deleted", root.row("providers", "lan").removable === false && root.row("providers", "free").removable === true)
      const models = root.table("models")
      root.check("Models lists the user's, then the presets", models.map(cells => cells[0]).join() === "local,gpt-6-luna,nova-3")
      root.check("a preset model says it is a preset", models[2][5] === I18n.t("models.preset") && models[0][5] === "")
      root.check("a preset waiting for a key says it is disabled", models[1][5] === I18n.t("models.preset_disabled") && root.row("models", "gpt-6-luna").dim === true)
      root.check("an enabled preset is not dim", root.row("models", "nova-3").dim === false)
    },
    () => {
      const view = root.window.view
      const section = view.sections[view.at.models]
      const facts = section.presetFacts("gpt-6-luna")
      root.check("a disabled preset gives the daemon's reason", facts.disabled === I18n.t("error.preset.key_missing", { name: "gpt-6-luna", variable: "OPENROUTER_API_KEY" }))
      const commands = section.commands(Eco.config, { id: "gpt-6-luna" })
      root.check("making a disabled preset a default is off, saying why", commands.every(command => command.enabled === false && command.tip === facts.disabled))
      const enabled = section.commands(Eco.config, { id: "nova-3" })
      root.check("an enabled preset may be made the default", enabled.length === 1 && enabled[0].enabled === true)
      view.picked = view.setOf(view.picked, view.at.models, "gpt-6-luna")
      const open = view.tableCommands(view.at.models).find(command => command.text === I18n.t("settings.edit"))
      root.check("EDIT is off on a preset, saying why", open.enabled === false && open.tip === I18n.t("presets.no_edit"))
      root.window.choose(view.at.secrets)
      view.open(view.at.secrets, "mine", true)
    },
    () => {
      const view = root.window.view
      root.check("a secret opens to be written", view.openPage !== null && view.openPage.key === "mine" && view.openPage.editing)
      const hint = platform => I18n.t(platform, { name: "MINE_KEY" })
      const shown = text => root.find(view, item => item.visible && item.text === text) !== null
      root.check("on Linux the hint is environment.d, set-environment and eco restart", shown(hint("secrets.set_linux")) && hint("secrets.set_linux").includes("environment.d/eco.conf") && hint("secrets.set_linux").includes("set-environment") && hint("secrets.set_linux").includes("eco restart"))
      Eco.handle({ type: "config", devices: [], omapass: { installed: true, page: "" }, credential_manager: true, platform: "windows",
        presets: Eco.presets, config: Eco.config })
    },
    () => {
      const view = root.window.view
      const text = I18n.t("secrets.set_windows", { name: "MINE_KEY" })
      root.check("on Windows the hint is setx", text.includes("setx MINE_KEY") && root.find(view, item => item.visible && item.text === text) !== null)
      root.check("on Windows the Credential Manager is offered in place of omapass", root.find(view, item => item.visible && item.text === I18n.t("provider.key_credential")) !== null
        && root.find(view, item => item.visible && item.text === I18n.t("provider.key_omapass")) === null)
      Eco.platform = "macos"
    },
    () => {
      const view = root.window.view
      const text = I18n.t("secrets.set_macos", { name: "MINE_KEY" })
      root.check("on macOS the hint is launchctl setenv", text.includes("launchctl setenv MINE_KEY") && root.find(view, item => item.visible && item.text === text) !== null)
      root.check("the hint names no key", !JSON.stringify(Eco.secretStatus).includes("sk-"))
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
