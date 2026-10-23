#pragma once

#include <QAbstractListModel>
#include <QHash>
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
    /// Adds the row of a send that the core has not reported yet (state "Starting") and returns its
    /// number; the first `update` for the id takes the row over.
    quint64 beginPending(const QString &id, QVariantMap row);
    /// Ends that pending send as Disconnected, unless the core took it over or a newer send to the
    /// same id began since.
    void failPending(const QString &id, quint64 number);
    /// Ends a send that is still "Starting" as Cancelled; false when the row is not in that state.
    bool cancelPending(const QString &id);
    /// True for states a transfer never leaves.
    static bool isFinal(const QString &state);
    bool hasActive() const;
    /// True while a transfer is live (not finished) and not just waiting for the user's approval:
    /// the Send page does not start another one meanwhile.
    bool busy() const { return m_busy; }
    /// Appends a finished transfer from an earlier session (below the live ones).
    void addHistory(const QVariantMap &info);
    void remove(const QString &id);
    void clearFinished();
    void clearAll();
    Q_INVOKABLE QVariantMap info(const QString &id) const;

signals:
    void busyChanged();
    /// A transfer reached a final state (once per transfer): `state` is Finished, Cancelled, Rejected or Disconnected.
    void transferEnded(const QString &id, const QString &state);

private:
    void updateBusy();

    struct Entry
    {
        QString id;
        QVariantMap info;
    };
    QList<Entry> m_entries;
    bool m_busy = false;
    QHash<QString, quint64> m_pendingNumber;
};
