import QtQml

// Singleton is the root of the overlay's singletons (Eco, Theme, I18n): an
// object that holds the objects declared in it, which QtObject cannot.
QtObject {
  default property list<QtObject> objects
}
