import QtQuick
import Quickshell
import Quickshell.Io

// Test-only driver, injected by scripts/shots/shoot into a copy of the overlay
// and never shipped: `run` evaluates a JS body with helpers to find, click and
// type into what is on screen, and to save a window as it is drawn.
QtObject {
  id: drive
  property var overlay
  property var settings

  function win(which) { return which === "settings" ? settings.item : overlay }
  // The window's root holds its content and the overlay layer where dialogs and menus open.
  function roots() {
    const out = [overlay.contentItem.parent]
    if (settings.item && settings.item.visible) out.push(settings.item.contentItem.parent)
    return out
  }
  function shown(item) {
    for (let p = item; p; p = p.parent)
      if (!p.visible || p.opacity <= 0.01) return false
    return item.width > 0 && item.height > 0
  }
  function label(item) {
    const parts = []
    for (const key of ["text", "label", "title", "placeholderText", "tip", "name", "icon", "value", "section"]) {
      try {
        const v = item[key]
        if (typeof v === "string" && v && !parts.includes(v)) parts.push(key + "=" + JSON.stringify(v))
      } catch (e) {}
    }
    return parts.join(" ")
  }
  function walk(item, out, depth) {
    if (!item || depth > 80) return
    out.push(item)
    const kids = item.children || []
    for (let i = 0; i < kids.length; i++) walk(kids[i], out, depth + 1)
  }
  function all() {
    const out = []
    for (const r of roots()) walk(r, out, 0)
    return out.filter(shown)
  }
  function kind(item) { return String(item).replace(/_QML.*|\(.*/, "").replace(/QQuick/, "") }
  function where(item) {
    const p = item.mapToItem(null, 0, 0)
    return Math.round(p.x) + "," + Math.round(p.y) + " " + Math.round(item.width) + "x" + Math.round(item.height)
  }
  // Every visible item that carries a text, label, tip or icon.
  function dump() {
    return all().filter(i => label(i)).map(i => kind(i) + " @" + where(i) + (i.enabled === false ? " [disabled]" : "") + " " + label(i)).join("\n")
  }
  function find(match, nth) {
    const re = new RegExp(match, "i")
    const hits = all().filter(i => re.test(label(i)))
    if (hits.length <= (nth || 0)) throw new Error("nothing matches " + match)
    return hits[nth || 0]
  }
  // Clicks the nearest control at or above the item matching `match`.
  function click(match, nth) {
    const hit = find(match, nth)
    for (let p = hit; p; p = p.parent) {
      if (typeof p.press === "function") { p.press(); return "pressed " + kind(p) }
      if (typeof p.clicked === "function") {
        try { p.clicked() } catch (e) { p.clicked({ x: 1, y: 1, button: Qt.LeftButton, accepted: true, modifiers: 0 }) }
        return "clicked " + kind(p)
      }
      for (const signal of ["picked", "chosen", "toggled", "activated", "triggered"])
        if (typeof p[signal] === "function") { p[signal](); return signal + " " + kind(p) }
    }
    throw new Error("no control around " + label(hit))
  }
  // Puts text in the input matching `match` (by placeholder, label or text) as typed.
  function type(match, text, nth) {
    const stack = [find(match, nth)]
    while (stack.length) {
      const i = stack.shift()
      if (i.cursorPosition !== undefined && i.text !== undefined && typeof i.selectAll === "function") {
        i.forceActiveFocus(); i.text = text
        if (typeof i.textEdited === "function") i.textEdited()
        return "typed into " + kind(i)
      }
      for (const c of i.children || []) stack.push(c)
    }
    throw new Error("no input under " + match)
  }
  // The first object of QML type `type` declared in the window, e.g. a dialog.
  function object(type, which) {
    const stack = [win(which).contentItem]
    while (stack.length) {
      const o = stack.shift()
      if (kind(o) === type) return o
      for (const c of o.data || []) stack.push(c)
    }
    throw new Error("no " + type)
  }
  // Takes the keyboard off any field, so no caret blinks in a picture.
  function blur(which) { win(which).contentItem.forceActiveFocus() }
  // Sizes a window in logical pixels, through the Qt window under it: an
  // offscreen window does not follow its implicit size once shown.
  function size(which, width, height) {
    const probe = Qt.createQmlObject("import QtQuick; Item { property var window: Window.window }", win(which).contentItem, "driveProbe")
    const window = probe.window
    probe.destroy()
    window.width = width
    window.height = height
    return window.width + "x" + window.height
  }
  function rect(item) {
    const p = item.mapToItem(null, 0, 0)
    return { x: p.x, y: p.y, w: item.width, h: item.height }
  }
  function inside(r, x, y) { return x >= r.x && x <= r.x + r.w && y >= r.y && y <= r.y + r.h }
  // Where an item can be seen: its rectangle cut by every clipping ancestor.
  function seen(item) {
    let r = rect(item)
    for (let p = item.parent; p; p = p.parent) {
      if (!p.clip) continue
      const c = rect(p)
      const x = Math.max(r.x, c.x), y = Math.max(r.y, c.y)
      r = { x: x, y: y, w: Math.max(0, Math.min(r.x + r.w, c.x + c.w) - x), h: Math.max(0, Math.min(r.y + r.h, c.y + c.h) - y) }
    }
    return r
  }
  // Visible items in the order they are painted: parents first, children by z.
  function painted(item, out) {
    out.push(item)
    const kids = (item.children || []).map((child, at) => ({ child, at }))
    kids.sort((a, b) => (a.child.z - b.child.z) || (a.at - b.at))
    for (const { child } of kids)
      if (child.visible && child.opacity > 0.01) painted(child, out)
    return out
  }
  function opaque(item) {
    if (item.color === undefined || item.border === undefined || item.radius === undefined || item.color.a < 0.5) return false
    let alpha = 1
    for (let p = item; p; p = p.parent) alpha *= p.opacity
    return alpha >= 0.5
  }
  // Offscreen, Qt Quick paints with its software renderer, which draws a Shape
  // (every icon, outline and trace here) past its clip and over what covers it.
  // The shapes the window would not show are hidden for the picture: those
  // wholly outside a clipping ancestor, icons cut by one, and those under an
  // opaque rectangle painted later. Returns them, to show again.
  function hideCovered(root) {
    const order = painted(root, [])
    const hidden = []
    order.forEach((item, at) => {
      if (kind(item) !== "Shape") return
      // What the shape draws, in the window: its paths' bounds, not its size.
      const b = item.boundingRect
      const from = item.mapToItem(null, b.x, b.y), to = item.mapToItem(null, b.x + b.width, b.y + b.height)
      const box = { x: from.x, y: from.y, w: to.x - from.x, h: to.y - from.y }
      const cx = box.x + box.w / 2, cy = box.y + box.h / 2
      let gone = false
      for (let p = item.parent; p && !gone; p = p.parent) {
        if (!p.clip) continue
        const c = rect(p)
        const outside = box.x >= c.x + c.w || box.y >= c.y + c.h || box.x + box.w <= c.x || box.y + box.h <= c.y
        const cut = box.x < c.x - 1 || box.y < c.y - 1 || box.x + box.w > c.x + c.w + 1 || box.y + box.h > c.y + c.h + 1
        gone = outside || (cut && Math.max(box.w, box.h) <= 48)
      }
      for (let i = at + 1; i < order.length && !gone; i++) {
        const other = order[i]
        let related = false
        for (let p = other; p && !related; p = p.parent) related = p === item
        for (let p = item; p && !related; p = p.parent) related = p === other
        gone = !related && opaque(other) && inside(seen(other), cx, cy)
      }
      if (gone) {
        hidden.push({ item, opacity: item.opacity })
        item.opacity = 0
      }
    })
    return hidden
  }
  // Saves the window as drawn, dialogs and menus included. The root has no QML
  // engine to grab with, so a recursive ShaderEffectSource of it, placed just
  // outside the window where it never shows, is grabbed instead, over a
  // backdrop in the window's colour.
  function shot(path, which) {
    const w = win(which)
    const root = w.contentItem.parent
    const made = name => root.children.find(c => c.objectName === name)
    const backdrop = made("driveBackdrop") || Qt.createQmlObject('import QtQuick; Rectangle { objectName: "driveBackdrop"; z: -1000 }', root, "driveBackdrop")
    backdrop.color = w.color
    backdrop.width = root.width
    backdrop.height = root.height
    const source = made("driveShot") || Qt.createQmlObject('import QtQuick; ShaderEffectSource { objectName: "driveShot"; recursive: true }', root, "driveShot")
    source.sourceItem = root
    source.sourceRect = Qt.rect(0, 0, root.width, root.height)
    source.x = root.width
    source.width = root.width
    source.height = root.height
    const hidden = hideCovered(root)
    const grabbing = source.grabToImage(result => {
      const saved = result.saveToFile(path)
      for (const { item, opacity } of hidden) item.opacity = opacity
      console.warn("drive shot", saved ? "saved" : "failed", path)
    })
    return grabbing ? w.width + "x" + w.height : "grab refused"
  }

  property IpcHandler ipc: IpcHandler {
    target: "drive"
    function run(code: string): string { return drive.run(code) }
  }
  function run(code) {
    try {
      const body = Function("Eco", "I18n", "Theme", "overlay", "settings", "d", code)
      const result = body(Eco, I18n, Theme, overlay, settings.item, drive)
      return result === undefined ? "ok" : (typeof result === "string" ? result : JSON.stringify(result))
    } catch (e) {
      return "error: " + e
    }
  }
}
