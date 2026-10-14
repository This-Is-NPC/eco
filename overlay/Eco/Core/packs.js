.pragma library

// The pack `text` holds, read again over `previous`: its texts, and those of
// `previous` it no longer has, so a window open while an update swaps the packs
// for another version's never shows a bare key.
function reread(previous, text) {
  return Object.assign({}, previous || {}, JSON.parse(text))
}
