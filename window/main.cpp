// eco-window <file.qml>: runs one entry of the overlay — shell.qml, the eco
// window, or lab.qml, the control lab — with the bridge in host.h,
// line_socket.h and text_file.h imported as `EcoHost`, and the entry's folder
// on the import path, where the Eco.* modules live (docs/design.md §3).

#include <QCoreApplication>
#include <QFileInfo>
#include <QGuiApplication>
#include <QQmlApplicationEngine>
#include <QUrl>
#include <QtQml>
#include <cstdio>

#include "host.h"
#include "line_socket.h"
#include "text_file.h"

int main(int argc, char **argv) {
  if (argc != 2) {
    std::fputs("usage: eco-window <file.qml>\n", stderr);
    return 2;
  }
  // Logs go to stderr, which the daemon passes on, never straight to the journal.
  qputenv("QT_FORCE_STDERR_LOGGING", "1");
  QGuiApplication app(argc, argv);
  app.setApplicationName(QStringLiteral("eco"));
  // Hyprland's class for every window, which packaging/hypr/eco.lua matches.
  QGuiApplication::setDesktopFileName(QStringLiteral("eco"));

  qmlRegisterSingletonType<Host>("EcoHost", 1, 0, "Host", [](QQmlEngine *, QJSEngine *) -> QObject * {
    return new Host;
  });
  qmlRegisterType<LineSocket>("EcoHost", 1, 0, "LineSocket");
  qmlRegisterType<TextFile>("EcoHost", 1, 0, "TextFile");

  QQmlApplicationEngine engine;
  QObject::connect(
      &engine, &QQmlApplicationEngine::objectCreationFailed, &app, [] { QCoreApplication::exit(1); },
      Qt::QueuedConnection);
  const QFileInfo entry(QString::fromLocal8Bit(argv[1]));
  engine.addImportPath(entry.absolutePath());
  engine.load(QUrl::fromLocalFile(entry.absoluteFilePath()));
  if (engine.rootObjects().isEmpty())
    return 1;
  return app.exec();
}
