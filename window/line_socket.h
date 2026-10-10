#pragma once

#include <QByteArray>
#include <QLocalSocket>
#include <QObject>
#include <QQmlParserStatus>
#include <QString>
#include <QTimer>

// LineSocket is one connection to the Unix socket at `path`, one line per
// message each way. While it is down it connects again every second; an empty
// path closes it.
class LineSocket : public QObject, public QQmlParserStatus {
  Q_OBJECT
  Q_INTERFACES(QQmlParserStatus)
  Q_PROPERTY(QString path READ path WRITE setPath NOTIFY pathChanged)
  Q_PROPERTY(bool connected READ connected NOTIFY connectedChanged)

public:
  explicit LineSocket(QObject *parent = nullptr) : QObject(parent) {
    retry.setInterval(1000);
    connect(&retry, &QTimer::timeout, this, &LineSocket::open);
    connect(&socket, &QLocalSocket::connected, this, [this] {
      retry.stop();
      Q_EMIT connectedChanged();
    });
    connect(&socket, &QLocalSocket::disconnected, this, [this] {
      buffer.clear();
      Q_EMIT connectedChanged();
      retry.start();
    });
    connect(&socket, &QLocalSocket::errorOccurred, this, [this] {
      if (!connected())
        retry.start();
    });
    connect(&socket, &QLocalSocket::readyRead, this, &LineSocket::read);
  }

  // The socket emits `disconnected` while it is destroyed, after the members its
  // handlers touch: none may run then.
  ~LineSocket() override { QObject::disconnect(&socket, nullptr, this, nullptr); }

  QString path() const { return target; }
  void setPath(const QString &next) {
    if (next == target)
      return;
    target = next;
    Q_EMIT pathChanged();
    socket.abort();
    if (complete)
      open();
  }

  bool connected() const { return socket.state() == QLocalSocket::ConnectedState; }

  // Sends `line` and a newline; nothing while it is not connected.
  Q_INVOKABLE void send(const QString &line) {
    if (!connected())
      return;
    socket.write(line.toUtf8() + '\n');
    socket.flush();
  }

  void classBegin() override {}
  // Connects once QML has set the path and the handlers.
  void componentComplete() override {
    complete = true;
    open();
  }

Q_SIGNALS:
  void pathChanged();
  void connectedChanged();
  void received(const QString &line);

private:
  void open() {
    if (target.isEmpty()) {
      retry.stop();
      return;
    }
    if (socket.state() == QLocalSocket::UnconnectedState)
      socket.connectToServer(target);
  }

  // Emits every whole line read; an empty one is skipped.
  void read() {
    buffer += socket.readAll();
    qsizetype end;
    while ((end = buffer.indexOf('\n')) >= 0) {
      const QByteArray line = buffer.left(end);
      buffer.remove(0, end + 1);
      if (!line.isEmpty())
        Q_EMIT received(QString::fromUtf8(line));
    }
  }

  QLocalSocket socket;
  QTimer retry;
  QByteArray buffer;
  QString target;
  bool complete = false;
};
