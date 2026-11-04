import QtQuick
import QtTest
import Eco.Core
import Eco.Settings

// The Secrets, Providers and Models tabs, run by tests/window.rs: each table
// lists the entries of the config, the ones eco ships among them as ordinary
// rows, a model that waits for a key is listed disabled with the reason, whether a secret resolves is told without a key, a secret or provider
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
        config: { rules: "", participants: [], kinds: ["meeting"], actions: [], contexts: [], context: { files: [] }, ui: { language: "auto" },
          secrets: [{ name: "mine", env: "MINE_KEY" }, { name: "DEEPGRAM", omapass: "deepgram" }, { name: "OPENROUTER", env: "OPENROUTER_API_KEY" },
            { name: "spare", env: "SPARE_KEY" }],
          providers: [{ name: "lan", base_url: "http://lan:8000/v1", secret: "mine" }, { name: "free", base_url: "http://free/v1" },
            { name: "OPENROUTER", base_url: "https://openrouter.ai/api/v1", secret: "OPENROUTER" },
            { name: "DEEPGRAM", base_url: "wss://api.deepgram.com/v1/listen", secret: "DEEPGRAM" }],
          models: [{ name: "local", type: "chat", provider: "lan", model: "qwen" },
            { name: "gpt-6-luna", type: "chat", provider: "OPENROUTER", model: "openai/gpt-6-luna", reasoning: "off" },
            { name: "nova-3", type: "transcription", provider: "DEEPGRAM", model: "nova-3" }],
          stt: { model: "", language: "auto", languages: ["auto"] }, llm: { model: "local", max_context_chars: 60000, concurrency: 8 } } })
      Eco.handle({ type: "secrets", status: {
        mine: { resolved: true },
        DEEPGRAM: { resolved: false, code: "secret.omapass_failed", params: { account: "deepgram", detail: "omapass: locked" } },
        OPENROUTER: { resolved: false, code: "secret.env_unset", params: { variable: "OPENROUTER_API_KEY" } } } })
      Eco.disabledModels = { "gpt-6-luna": { code: "model.disabled", params: { name: "gpt-6-luna", variable: "OPENROUTER_API_KEY" } } }
      Eco.configOpen = true
    },
    () => {
      const secrets = root.table("secrets")
      root.check("Secrets lists every secret of the config, shipped or not", secrets.map(cells => cells[0]).join() === "mine,DEEPGRAM,OPENROUTER,spare")
      root.check("a secret that resolves says so, without a key", secrets[0][3] === I18n.t("secrets.resolved") && secrets[0][2] === "MINE_KEY")
      root.check("a secret that does not says why", secrets[1][3] === I18n.t("error.secret.omapass_failed", { account: "deepgram", detail: "omapass: locked" }))
      root.check("a secret that does not resolve says why", secrets[2][3] === I18n.t("error.secret.env_unset", { variable: "OPENROUTER_API_KEY" }))
      root.check("every secret is an ordinary row", root.table("secrets").every((cells, i) => root.row("secrets", cells[0]).fixed !== true && root.row("secrets", cells[0]).edits !== false))
      root.check("a secret a provider uses is not deleted", root.row("secrets", "mine").removable === false && root.row("secrets", "OPENROUTER").removable === false)
      root.check("a secret nothing uses is", root.row("secrets", "spare").removable === true)
      const providers = root.table("providers")
      root.check("Providers lists every provider of the config", providers.map(cells => cells[0]).join() === "lan,free,OPENROUTER,DEEPGRAM")
      root.check("a provider shows its secret or none", providers[0][2] === "mine" && providers[1][2] === I18n.t("providers.no_secret"))
      root.check("a provider shows what uses it", providers[0][3] === "local" && providers[2][3] === "gpt-6-luna")
      root.check("a provider a model uses is not deleted", root.row("providers", "lan").removable === false && root.row("providers", "free").removable === true)
      const models = root.table("models")
      root.check("Models lists every model of the config", models.map(cells => cells[0]).join() === "local,gpt-6-luna,nova-3")
      root.check("a model waiting for a key says it is disabled and is dim", models[1][5] === I18n.t("models.disabled") && root.row("models", "gpt-6-luna").dim === true)
      root.check("an enabled model says nothing and is not dim", models[2][5] === "" && models[0][5] === "" && root.row("models", "nova-3").dim === false)
      root.check("a model nothing makes a default is deleted, the default is not", root.row("models", "gpt-6-luna").removable === true && root.row("models", "local").removable === false)
    },
    () => {
      const view = root.window.view
      const section = view.sections[view.at.models]
      const reason = I18n.t("error.model.disabled", { name: "gpt-6-luna", variable: "OPENROUTER_API_KEY" })
      root.check("a disabled model gives the daemon's reason", section.disabledText("gpt-6-luna") === reason)
      const commands = section.commands(Eco.config, { id: "gpt-6-luna" })
      root.check("making a disabled model a default is off, saying why", commands.every(command => command.enabled === false && command.tip === reason))
      const enabled = section.commands(Eco.config, { id: "nova-3" })
      root.check("an enabled model may be made the default", enabled.length === 1 && enabled[0].enabled === true)
      view.picked = view.setOf(view.picked, view.at.models, "gpt-6-luna")
      const bar = view.tableCommands(view.at.models)
      const open = bar.find(command => command.text === I18n.t("settings.edit"))
      root.check("EDIT is on for a disabled model", open.enabled === true)
      root.check("DELETE is on for a model nothing uses", bar.find(command => command.text === I18n.t("settings.delete")).enabled === true)
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
        config: Eco.config })
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
      const section = view.sections[view.at.secrets]
      const check = id => section.commands(Eco.config, { id: id })[0]
      const told = () => Eco.notices.map(notice => Eco.noticeText(notice))
      Eco.notices = []
      root.check("CHECK is idle before it is pressed", check("mine").busy === false)
      check("mine").run()
      root.check("CHECK is busy while the reply is awaited, only for its secret", check("mine").busy === true && check("DEEPGRAM").busy === false)
      Eco.handle({ type: "secrets", status: Eco.secretStatus })
      root.check("CHECK on a resolving secret tells a key is read, once", told().length === 1 && told()[0] === I18n.t("secrets.checked_ok", { name: "mine" }))
      root.check("CHECK is idle again after the reply", check("mine").busy === false)
      Eco.handle({ type: "secrets", status: Eco.secretStatus })
      root.check("a reply that CHECK did not ask for tells nothing", told().length === 1)
      Eco.notices = []
      check("OPENROUTER").run()
      Eco.handle({ type: "secrets", status: Eco.secretStatus })
      const missing = told()
      root.check("CHECK on a missing secret gives the reason and how to set it", missing.length === 1
        && missing[0].includes(I18n.t("error.secret.env_unset", { variable: "OPENROUTER_API_KEY" }))
        && missing[0].includes(I18n.t("secrets.set_macos", { name: "OPENROUTER_API_KEY" })))
      Eco.notices = []
      check("DEEPGRAM").run()
      Eco.handle({ type: "secrets", status: Eco.secretStatus })
      root.check("CHECK on an omapass secret gives the reason alone", told().length === 1
        && told()[0].includes(I18n.t("error.secret.omapass_failed", { account: "deepgram", detail: "omapass: locked" })) && !told()[0].includes("setenv"))
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
