import QtQuick
import QtQuick.Shapes

// Icon is eco's one icon set: thin line drawings on a 16-unit grid, one
// stroke weight, round ends, so every glyph matches the hairline UI instead of
// borrowing whatever a fallback font draws.
Item {
  id: icon

  property string name
  property color color: Theme.foreground
  property real size: 14
  // Stroke weight in screen pixels, the same at every size.
  property real weight: 1.25

  readonly property var paths: ({
    settings: "M2 4.5h7 M12 4.5h2 M10.5 3v3 M2 11.5h2 M7 11.5h7 M5.5 10v3",
    play: "M5 3.5v9l7.5-4.5z",
    pause: "M5.5 3.5v9 M10.5 3.5v9",
    stop: "M4 4h8v8h-8z",
    list: "M3 4.5h10 M3 8h10 M3 11.5h10",
    import: "M8 2.5v8 M4.5 7l3.5 3.5 3.5-3.5 M3 13.5h10",
    edit: "M3 4h6 M3 7h5 M3 10h6 M12 3.5v9 M10.5 3.5h3 M10.5 12.5h3",
    close: "M4.5 4.5l7 7 M11.5 4.5l-7 7",
    trash: "M2.5 4.5h11 M6 4.5v-2h4v2 M4 4.5l0.7 9h6.6l0.7-9 M6.8 7v4 M9.2 7v4",
    copy: "M6 6h7v7h-7z M3.5 10.5v-7h7",
    "chevron-down": "M4.5 6.5l3.5 3.5 3.5-3.5",
    "chevron-up": "M4.5 9.5l3.5-3.5 3.5 3.5",
    "chevron-left": "M9.5 4.5l-3.5 3.5 3.5 3.5",
    "chevron-right": "M6.5 4.5l3.5 3.5-3.5 3.5",
    enter: "M12.5 3.5v4.5c0 1-0.5 1.5-1.5 1.5h-7.5 M6 7l-2.5 2.5 2.5 2.5",
    warning: "M8 2.5l6 11h-12z M8 6.5v3.5 M8 12v0.01",
    spark: "M8 3l1.4 3.6 3.6 1.4-3.6 1.4-1.4 3.6-1.4-3.6-3.6-1.4 3.6-1.4z",
    people: "M4 5a2 2 0 1 0 4 0a2 2 0 1 0 -4 0 M2.5 13v-0.5a3.5 3.5 0 0 1 7 0v0.5 M10.5 3.2a2 2 0 0 1 0 3.6 M11.5 9.3a3.5 3.5 0 0 1 2 3.2v0.5",
    plus: "M8 3.5v9 M3.5 8h9",
    folder: "M2.5 4.5v8h11v-6.5h-6l-1.5-1.5z",
    more: "M3.5 8h0.01 M8 8h0.01 M12.5 8h0.01",
    check: "M3.5 8.5l3 3 6-7",
    send: "M13.5 2.5l-11 4.5 4.5 2 2 4.5z M7 9l6.5-6.5",
    translate: "M2 3.5h7 M5.5 2v1.5 M3.5 3.5c0.6 2.2 2.2 3.8 4.5 4.8 M7.5 3.5c-0.6 2.2-2.2 3.8-4.5 4.8 M8.5 14l2.5-6.5 2.5 6.5 M9.3 12h3.4",
    merge: "M4 2.5v3c0 2.5 4 2.5 4 5v3 M12 2.5v3c0 2.5-4 2.5-4 5",
    color: "M8 2.5c2.5 3 4 5.2 4 7.2a4 4 0 0 1 -8 0c0-2 1.5-4.2 4-7.2z",
    info: "M2.5 8a5.5 5.5 0 1 0 11 0a5.5 5.5 0 1 0 -11 0 M8 7.5v3.5 M8 5.2v0.01"
  })

  implicitWidth: size
  implicitHeight: size

  Shape {
    width: 16
    height: 16
    preferredRendererType: Shape.CurveRenderer
    transform: Scale { xScale: icon.size / 16; yScale: icon.size / 16 }

    ShapePath {
      strokeColor: icon.color
      strokeWidth: icon.weight * 16 / icon.size
      fillColor: "transparent"
      capStyle: ShapePath.RoundCap
      joinStyle: ShapePath.RoundJoin
      PathSvg { path: icon.paths[icon.name] || "" }
    }
  }
}
