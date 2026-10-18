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
    };

    using QAbstractListModel::QAbstractListModel;

    int rowCount(const QModelIndex &parent = {}) const override;
    QVariant data(const QModelIndex &index, int role) const override;
    QHash<int, QByteArray> roleNames() const override;

    /// `trust` is "", "kuickshare" (runs KuickShare) or "trusted" (paired).
    void upsert(const QString &id, const QString &name, const QString &type, const QString &address, const QString &trust);
    void remove(const QString &id);
    void clear();

private:
    struct Entry
    {
        QString id;
        QString name;
        QString type;
        QString address;
        QString trust;
    };
    int indexOf(const QString &id) const;

    QList<Entry> m_entries;
};
