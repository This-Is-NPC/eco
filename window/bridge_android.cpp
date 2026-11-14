#include "bridge_android.h"

#include <QCoreApplication>
#include <QDir>
#include <QJniObject>
#include <QPermissions>

namespace {
const char service[] = "eco/app/DaemonService";

QJniObject context() { return QNativeInterface::QAndroidApplication::context(); }

QString filesDir() {
  return context().callObjectMethod("getFilesDir", "()Ljava/io/File;").callObjectMethod<jstring>("getAbsolutePath").toString();
}

void startService() {
  QJniObject::callStaticMethod<void>(service, "start", "(Landroid/content/Context;Ljava/lang/String;Ljava/lang/String;)V",
                                     context().object(), QJniObject::fromString(AndroidBridge::dataDir()).object(),
                                     QJniObject::fromString(AndroidBridge::runtimeDir()).object());
}
} // namespace

namespace AndroidBridge {
QString dataDir() { return QDir(filesDir()).filePath(QStringLiteral("eco")); }

QString runtimeDir() { return QDir(filesDir()).filePath(QStringLiteral("run")); }

QString socketPath() { return QDir(runtimeDir()).filePath(QStringLiteral("eco.sock")); }

void start() {
  qputenv("ECO_SOCKET", socketPath().toLocal8Bit());
  qApp->requestPermission(QMicrophonePermission{}, qApp, [](const QPermission &microphone) {
    // A microphone service needs the microphone allowed to start at all.
    if (microphone.status() != Qt::PermissionStatus::Granted) {
      qWarning("the microphone is not allowed: the daemon is not started");
      return;
    }
    QJniObject::callStaticMethod<void>(service, "askNotifications", "(Landroid/app/Activity;)V", context().object());
    startService();
  });
}
} // namespace AndroidBridge
