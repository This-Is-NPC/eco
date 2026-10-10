#pragma once

#include <QFile>
#include <QFileSystemWatcher>
#include <QObject>
#include <QQmlParserStatus>
#include <QString>

// TextFile is the file or directory at `path`, watched: `text` holds the file's
// contents as last read ("" for a directory or a missing file), and `changed`
// says the file or directory changed on disk, which `reload` reads again.
class TextFile : public QObject, public QQmlParserStatus {
  Q_OBJECT
  Q_INTERFACES(QQmlParserStatus)
  Q_PROPERTY(QString path READ path WRITE setPath NOTIFY pathChanged)
  Q_PROPERTY(QString text READ text NOTIFY textChanged)

public:
  explicit TextFile(QObject *parent = nullptr) : QObject(parent) {
    connect(&watcher, &QFileSystemWatcher::fileChanged, this, &TextFile::changed);
    connect(&watcher, &QFileSystemWatcher::directoryChanged, this, &TextFile::changed);
  }

  QString path() const { return target; }
  void setPath(const QString &next) {
    if (next == target)
      return;
    if (!target.isEmpty())
      watcher.removePath(target);
    target = next;
    Q_EMIT pathChanged();
    if (complete)
      reload();
  }

  QString text() const { return contents; }

  // Reads the file again, and watches it again when it was replaced.
  Q_INVOKABLE void reload() {
    QFile file(target);
    const QString next = file.open(QIODevice::ReadOnly) ? QString::fromUtf8(file.readAll()) : QString();
    if (!target.isEmpty() && QFile::exists(target) && !watcher.files().contains(target) &&
        !watcher.directories().contains(target))
      watcher.addPath(target);
    if (next != contents) {
      contents = next;
      Q_EMIT textChanged();
    }
  }

  void classBegin() override {}
  // Reads once QML has set the path and the handlers.
  void componentComplete() override {
    complete = true;
    reload();
  }

Q_SIGNALS:
  void pathChanged();
  void textChanged();
  void changed();

private:
  QFileSystemWatcher watcher;
  QString target;
  QString contents;
  bool complete = false;
};
