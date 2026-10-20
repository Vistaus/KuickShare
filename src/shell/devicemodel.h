#pragma once

#include <QAbstractListModel>
#include <QList>

/// Nearby devices reported by the core's discovery.
class DeviceModel : public QAbstractListModel
{
    Q_OBJECT

public:
    enum Role
    {
        IdRole = Qt::UserRole + 1,
        NameRole,
        TypeRole,
        AddressRole,
        TrustRole,
        MediumRole,
    };

    using QAbstractListModel::QAbstractListModel;

    int rowCount(const QModelIndex &parent = {}) const override;
    QVariant data(const QModelIndex &index, int role) const override;
    QHash<int, QByteArray> roleNames() const override;

    /// `medium` is "lan" or "ble"; `address` is empty for "ble".
    /// `trust` is "", "kuickshare" (runs KuickShare) or "trusted" (paired).
    void upsert(const QString &id, const QString &name, const QString &type, const QString &address, const QString &trust, const QString &medium);
    void remove(const QString &id);
    /// The name of a known device, or an empty string.
    QString nameOf(const QString &id) const;
    void clear();

private:
    struct Entry
    {
        QString id;
        QString name;
        QString type;
        QString address;
        QString trust;
        QString medium;
    };
    int indexOf(const QString &id) const;

    QList<Entry> m_entries;
};
