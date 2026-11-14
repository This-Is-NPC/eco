#include "bridge_android.h"

#include <QCoreApplication>
#include <QDir>
#include <QJniEnvironment>
#include <QJniObject>
#include <QPermissions>

namespace {
const char service[] = "eco/app/DaemonService";

bool serviceStarted = false;

QJniObject context() { return QNativeInterface::QAndroidApplication::context(); }

QString filesDir() {
  return context().callObjectMethod("getFilesDir", "()Ljava/io/File;").callObjectMethod<jstring>("getAbsolutePath").toString();
}

void startService() {
  QJniObject::callStaticMethod<void>(service, "start", "(Landroid/content/Context;Ljava/lang/String;Ljava/lang/String;)V",
                                     context().object(), QJniObject::fromString(AndroidBridge::dataDir()).object(),
                                     QJniObject::fromString(AndroidBridge::runtimeDir()).object());
}

void reportNative(JNIEnv *, jclass, jstring status) { AndroidBridge::report(QJniObject(status).toString()); }

// The service's reports come through a native method of its class.
void listenToService() {
  QJniEnvironment env;
  if (!env.registerNativeMethods(service, {{"report", "(Ljava/lang/String;)V", reinterpret_cast<void *>(&reportNative)}}))
    qWarning("the service's reports are not heard: the window cannot say why the daemon is down");
}

// A microphone service needs the microphone allowed to start at all: starts the
// service once it is, and not twice.
void begin() {
  if (serviceStarted)
    return;
  serviceStarted = true;
  AndroidBridge::Status::instance().set({});
  QJniObject::callStaticMethod<void>(service, "askNotifications", "(Landroid/app/Activity;)V", context().object());
  startService();
}

void answered(const QPermission &microphone) {
  if (microphone.status() == Qt::PermissionStatus::Granted)
    begin();
  else
    qWarning("the microphone is not allowed: the daemon is not started");
}
} // namespace

namespace AndroidBridge {
Status &Status::instance() {
  static Status status;
  return status;
}

void Status::set(const QString &status) {
  if (status == current)
    return;
  current = status;
  qInfo("status: %s", qPrintable(status));
  Q_EMIT changed();
}

QString dataDir() { return QDir(filesDir()).filePath(QStringLiteral("eco")); }

QString runtimeDir() { return QDir(filesDir()).filePath(QStringLiteral("run")); }

QString socketPath() { return QDir(runtimeDir()).filePath(QStringLiteral("eco.sock")); }

void report(const QString &status) {
  QMetaObject::invokeMethod(&Status::instance(), [status] { Status::instance().set(status); }, Qt::QueuedConnection);
}

void start() {
  qputenv("ECO_SOCKET", socketPath().toLocal8Bit());
  listenToService();
  qApp->requestPermission(QMicrophonePermission{}, qApp, answered);
}
} // namespace AndroidBridge
