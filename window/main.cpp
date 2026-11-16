// eco-window <file.qml>: runs one entry of the overlay — shell.qml, the eco
// window, tray.qml, the tray icon, or lab.qml, the control lab — with the
// bridge in host.h, line_socket.h and text_file.h imported as `EcoHost`, and
// the entry's folder on the import path, where the Eco.* modules live
// (docs/design.md §3). On macOS, opened from Finder or Launchpad with no
// file, it starts eco with its window instead, as eco-launch does on Windows.

#include <QApplication>
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

#ifdef Q_OS_ANDROID
#include "bridge_android.h"
#endif

#ifdef Q_OS_MACOS
#include <QDir>
#include <QProcess>
#endif

int main(int argc, char **argv) {
#ifdef Q_OS_MACOS
  // The installed bundle is lib/eco/eco.app, and its program
  // Contents/MacOS/eco: the eco beside it is bin/eco, five folders up.
  if (argc == 1) {
    QCoreApplication core(argc, argv);
    const QString eco = QDir(QCoreApplication::applicationDirPath()).filePath(QStringLiteral("../../../../../bin/eco"));
    return QProcess::startDetached(QDir::cleanPath(eco), {QStringLiteral("start")}) ? 0 : 1;
  }
#endif
#ifndef Q_OS_ANDROID
  if (argc != 2) {
    std::fputs("usage: eco-window <file.qml>\n", stderr);
    return 2;
  }
#endif
#ifndef Q_OS_ANDROID
  // Logs go to stderr, which the daemon passes on, never straight to the journal;
  // on Android they go to logcat, where the app's are read.
  qputenv("QT_FORCE_STDERR_LOGGING", "1");
#endif
#ifdef Q_OS_WIN
  // The tray icon is native on Windows, and only a native menu opens on its
  // right click; a platform chosen from outside (offscreen tests) is kept.
  if (!qEnvironmentVariableIsSet("QT_QPA_PLATFORM"))
    qputenv("QT_QPA_PLATFORM", "windows:menus=native");
#endif
  // A QApplication, not a QGuiApplication: the tray's menu is a widget where
  // the platform has no native one.
  QApplication app(argc, argv);
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
#ifdef Q_OS_ANDROID
  AndroidBridge::start();
  // The activity gives no arguments: the entry is the phone's window, in the
  // resources of the APK.
  engine.addImportPath(QStringLiteral("qrc:/overlay"));
  engine.load(QUrl(QStringLiteral("qrc:/overlay/phone.qml")));
#else
  const QFileInfo entry(QString::fromLocal8Bit(argv[1]));
  engine.addImportPath(entry.absolutePath());
  engine.load(QUrl::fromLocalFile(entry.absoluteFilePath()));
#endif
  if (engine.rootObjects().isEmpty())
    return 1;
  return app.exec();
}
