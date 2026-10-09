#pragma once

#include <QObject>
#include <QProcess>
#include <QString>
#include <QStringList>

// Host is the process as QML sees it: its environment and the clipboard.
class Host : public QObject {
  Q_OBJECT

public:
  using QObject::QObject;

  // The value of the environment variable `name`, or "" when it is unset.
  Q_INVOKABLE QString env(const QString &name) const { return qEnvironmentVariable(name.toUtf8().constData()); }

  // Puts `text` on the Wayland clipboard through wl-copy, which keeps serving it
  // after this process exits.
  Q_INVOKABLE void copy(const QString &text) const {
    QProcess::startDetached(QStringLiteral("wl-copy"), {QStringLiteral("--"), text});
  }
};
