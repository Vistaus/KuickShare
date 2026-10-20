#pragma once

#include <QAbstractListModel>
#include <QList>
#include <QVariantMap>

/// One row per transfer id, updated in place from core `TransferUpdated` signals.
class TransferModel : public QAbstractListModel
{
    Q_OBJECT

public:
    enum Role
    {
        IdRole = Qt::UserRole + 1,
        KindRole,
        StateRole,
        DeviceRole,
        PinRole,
        PreviewRole,
        TotalBytesRole,
        AckBytesRole,
        FilesRole,
        TextRole,
        PayloadKindRole,
        TimeRole,
        TrustedRole,
        AlwaysAcceptedRole,
        HistoryUidRole,
    };

    using QAbstractListModel::QAbstractListModel;

    int rowCount(const QModelIndex &parent = {}) const override;
    QVariant data(const QModelIndex &index, int role) const override;
    QHash<int, QByteArray> roleNames() const override;

    void update(const QString &id, const QVariantMap &info);
    /// True for states a transfer never leaves.
    static bool isFinal(const QString &state);
    bool hasActive() const;
    /// Appends a finished transfer from an earlier session (below the live ones).
    void addHistory(const QVariantMap &info);
    void remove(const QString &id);
    void clearFinished();
    void clearAll();
    Q_INVOKABLE QVariantMap info(const QString &id) const;

private:
    struct Entry
    {
        QString id;
        QVariantMap info;
    };
    QList<Entry> m_entries;
};
