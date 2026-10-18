#include "dndwatcher.h"

#include <QDBusConnection>
#include <QDBusMessage>
#include <QDBusPendingCallWatcher>
#include <QDBusPendingReply>
#include <QDBusServiceWatcher>
#include <QDBusVariant>

#include "corebridge.h"

namespace
{
    const QString service = QStringLiteral("org.freedesktop.Notifications");
    const QString path = QStringLiteral("/org/freedesktop/Notifications");
    const QString propsIface = QStringLiteral("org.freedesktop.DBus.Properties");
}

DndWatcher::DndWatcher(CoreBridge *core, QObject *parent)
    : QObject(parent), m_core(core)
{
    auto bus = QDBusConnection::sessionBus();
    bus.connect(service, path, propsIface, QStringLiteral("PropertiesChanged"), this, SLOT(onPropertiesChanged(QString, QVariantMap, QStringList)));

    // The notification server restarts with the Plasma shell: read the state again.
    auto *watcher = new QDBusServiceWatcher(service, bus, QDBusServiceWatcher::WatchForRegistration, this);
    connect(watcher, &QDBusServiceWatcher::serviceRegistered, this, &DndWatcher::readInhibited);

    connect(core, &CoreBridge::hideWhenDndChanged, this, &DndWatcher::apply);
    // The core starts visible: tell it again whenever it (re)appears.
    connect(core, &CoreBridge::connectedChanged, this, &DndWatcher::apply);
    readInhibited();
}

void DndWatcher::readInhibited()
{
    auto msg = QDBusMessage::createMethodCall(service, path, propsIface, QStringLiteral("Get"));
    msg << service << QStringLiteral("Inhibited");
    auto *watcher = new QDBusPendingCallWatcher(QDBusConnection::sessionBus().asyncCall(msg), this);
    connect(watcher, &QDBusPendingCallWatcher::finished, this, [this](QDBusPendingCallWatcher *w)
            {
        w->deleteLater();
        const QDBusPendingReply<QDBusVariant> reply = *w;
        if (!reply.isError()) {
            m_dnd = reply.value().variant().toBool();
            apply();
        } });
}

void DndWatcher::onPropertiesChanged(const QString &interfaceName, const QVariantMap &changed, const QStringList &invalidated)
{
    if (interfaceName != service)
    {
        return;
    }
    if (changed.contains(QStringLiteral("Inhibited")))
    {
        m_dnd = changed.value(QStringLiteral("Inhibited")).toBool();
        apply();
    }
    else if (invalidated.contains(QStringLiteral("Inhibited")))
    {
        readInhibited();
    }
}

void DndWatcher::apply()
{
    if (m_core->connected())
    {
        m_core->setSuspended(m_dnd && m_core->hideWhenDnd());
    }
}
