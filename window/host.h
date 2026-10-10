#pragma once

#include <QObject>
#include <QString>
#include <QUrl>

#ifdef Q_OS_WIN
#include <QClipboard>
#include <QGuiApplication>
#else
#include <QProcess>
#include <QStringList>
#endif

// Host is the process as QML sees it: its environment, local paths and the
// clipboard.
class Host : public QObject {
  Q_OBJECT

public:
  using QObject::QObject;

  // The value of the environment variable `name`, or "" when it is unset.
  Q_INVOKABLE QString env(const QString &name) const { return qEnvironmentVariable(name.toUtf8().constData()); }

  // The local path a file: URL names, as file dialogs and drops give them.
  Q_INVOKABLE QString localPath(const QUrl &url) const { return url.toLocalFile(); }

  // Puts `text` on the clipboard so that it stays there after this process
  // exits: on Windows through Qt, which hands it to the system as the process
  // exits; elsewhere through wl-copy, which keeps serving it.
  Q_INVOKABLE void copy(const QString &text) const {
#ifdef Q_OS_WIN
    QGuiApplication::clipboard()->setText(text);
#else
    QProcess::startDetached(QStringLiteral("wl-copy"), {QStringLiteral("--"), text});
#endif
  }

};
