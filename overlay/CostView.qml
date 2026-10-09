pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Layouts

// CostView is what one session cost, on its own screen, opened from its
// details: the total, what the LLM (answers, reviews, translations) and the
// transcription each add up to, why a part is unknown or estimated, and every
// charge in its log, newest first. A live session's follows each new charge.
// Keys: Esc goes back to the session.
ColumnLayout {
  id: view

  property bool active: false
  // {llm_usd, llm_unknown, transcription_usd, transcription_unknown, total_usd,
  // parts: [{for, usd, unknown}], items: [{for, at, model, usd, unknown,
  // estimate, seconds, prompt, count}]}, or null.
  readonly property var cost: Eco.cost
  readonly property var items: cost ? cost.items : []
  // The audio the transcription charges stand for.
  readonly property real minutes: items.reduce((sum, item) => sum + (item.for === "transcription" ? item.seconds : 0), 0) / 60
  // Why the unknown charges are unknown, each once.
  readonly property var reasons: items.map(item => item.unknown).filter((code, i, all) => code && all.indexOf(code) === i)

  spacing: 12
  focus: active
  onActiveChanged: if (active) table.focusRows()
  Keys.onEscapePressed: backButton.press()

  function amount(usd, unknown) { return I18n.money(usd, unknown) }
  // How long was heard: seconds under a minute, else minutes.
  function heard(seconds) {
    return seconds < 60
      ? I18n.t("cost.seconds", { n: Math.round(seconds) })
      : I18n.t("cost.minutes", { n: (seconds / 60).toLocaleString(I18n.locale, "f", seconds < 600 ? 1 : 0) })
  }
  // What a charge paid for: the answer's question or action, the minutes heard,
  // or how many answers kept no cost.
  function what(item) {
    if (item.count !== null)
      return I18n.t("cost.untracked", { n: item.count })
    const kind = I18n.t("cost.for." + item.for)
    if (item.prompt !== null)
      return I18n.t("cost.asked", { what: kind, prompt: item.prompt })
    if (item.seconds !== null)
      return I18n.t("cost.heard", { what: kind, heard: heard(item.seconds) })
    return kind
  }
  function price(item) {
    if (item.usd === null)
      return I18n.t("cost.why." + item.unknown)
    const usd = amount(item.usd, false)
    return item.estimate ? I18n.t("cost.estimate", { amount: usd }) : usd
  }
  function part(what) {
    const found = (view.cost ? view.cost.parts : []).find(p => p.for === what)
    return found ? I18n.t("cost.part." + what, { amount: amount(found.usd, found.unknown) }) : ""
  }

  RowLayout {
    Layout.fillWidth: true
    spacing: 10
    TraceButton {
      id: backButton
      dense: true
      role: "navigation"
      icon: "chevron-left"
      text: I18n.t("cost.back")
      onClicked: Eco.closeCost()
    }
    Item { Layout.fillWidth: true }
    Caption { text: I18n.t("cost.total") }
    Label {
      text: view.cost ? view.amount(view.cost.total_usd, view.cost.llm_unknown || view.cost.transcription_unknown) : ""
      color: Theme.primary
      font.pixelSize: 16
    }
  }

  // What each part adds up to, then why any is unknown or estimated.
  ColumnLayout {
    Layout.fillWidth: true
    visible: view.cost !== null
    spacing: 4
    // A part never breaks across lines: a narrow window wraps between them.
    Flow {
      Layout.fillWidth: true
      spacing: 4
      Repeater {
        model: view.cost ? [
          I18n.t("cost.llm", { amount: view.amount(view.cost.llm_usd, view.cost.llm_unknown) }),
          ...["answer", "review", "translation"].map(what => view.part(what)).filter(text => text)
        ] : []
        delegate: Label {
          required property string modelData
          required property int index
          text: modelData
          rightPadding: 10
          color: index === 0 ? Theme.foreground : Theme.dim
          font.pixelSize: 10
          font.letterSpacing: 1
        }
      }
    }
    Label {
      Layout.fillWidth: true
      text: view.cost ? I18n.t(view.minutes > 0 ? "cost.transcription_timed" : "cost.transcription", {
        heard: view.heard(view.minutes * 60).toUpperCase(),
        amount: view.amount(view.cost.transcription_usd, view.cost.transcription_unknown)
      }) : ""
      font.pixelSize: 10
      font.letterSpacing: 1
      wrapMode: Text.Wrap
    }
    Repeater {
      model: view.reasons
      delegate: Label {
        required property string modelData
        Layout.fillWidth: true
        text: I18n.t("cost.reason." + modelData)
        color: Theme.warning
        font.pixelSize: 10
        wrapMode: Text.Wrap
      }
    }
    Label {
      Layout.fillWidth: true
      visible: view.items.some(item => item.estimate)
      text: I18n.t("cost.estimated")
      color: Theme.dim
      font.pixelSize: 10
      wrapMode: Text.Wrap
    }
  }

  DataTable {
    id: table
    Layout.fillWidth: true
    Layout.fillHeight: true
    active: view.active
    rows: view.items
    loading: view.cost === null
    idOf: item => [item.for, item.at, item.model, item.count].join("|")
    columns: [
      { title: I18n.t("cost.column.what"), weight: 0.42 },
      { title: I18n.t("cost.column.time"), weight: 0.12 },
      { title: I18n.t("cost.column.model"), weight: 0.22 },
      { title: I18n.t("cost.column.cost"), weight: 0.24 }
    ]
    cells: item => [view.what(item), item.at !== null ? I18n.date(item.at, I18n.t("day.time")) : "", item.model || "", view.price(item)]
    tone: (item, column) => column === 3 && item.usd === null ? Theme.warning : ""
    stacked: [[0], [1, 2]]
    status: 3
    emptyText: I18n.t("cost.empty")
    onBackRequested: backButton.press()
  }
}
