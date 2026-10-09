.pragma library

// Whether `item` is `ancestor` or inside it.
function within(item, ancestor) {
  for (let at = item; at; at = at.parent)
    if (at === ancestor)
      return true
  return false
}
