#pragma once

#include <QObject>
#include <QStringList>
#include <QVariantMap>

class CoreBridge;

/// Follows Do Not Disturb mode and hides this computer from other devices while it is on
class DndWatcher : public QObject
{
    Q_OBJECT

public:
    explicit DndWatcher(CoreBridge *core, QObject *parent = nullptr);

private Q_SLOTS:
    void onPropertiesChanged(const QString &interfaceName, const QVariantMap &changed, const QStringList &invalidated);

private:
    void readInhibited();
    void apply();

    CoreBridge *m_core;
    bool m_dnd = false;
};
