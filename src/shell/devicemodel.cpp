#include "devicemodel.h"

int DeviceModel::rowCount(const QModelIndex &parent) const
{
    return parent.isValid() ? 0 : m_entries.size();
}

QVariant DeviceModel::data(const QModelIndex &index, int role) const
{
    if (!checkIndex(index, CheckIndexOption::IndexIsValid))
    {
        return {};
    }
    const Entry &entry = m_entries.at(index.row());
    switch (role)
    {
    case IdRole:
        return entry.id;
    case NameRole:
        return entry.name;
    case TypeRole:
        return entry.type;
    case AddressRole:
        return entry.address;
    case TrustRole:
        return entry.trust;
    }
    return {};
}

QHash<int, QByteArray> DeviceModel::roleNames() const
{
    return {
        {IdRole, "deviceId"},
        {NameRole, "name"},
        {TypeRole, "deviceType"},
        {AddressRole, "address"},
        {TrustRole, "trust"},
    };
}

int DeviceModel::indexOf(const QString &id) const
{
    for (int row = 0; row < m_entries.size(); ++row)
    {
        if (m_entries[row].id == id)
        {
            return row;
        }
    }
    return -1;
}

void DeviceModel::upsert(const QString &id, const QString &name, const QString &type, const QString &address, const QString &trust)
{
    const Entry entry{id, name, type, address, trust};
    if (const int row = indexOf(id); row >= 0)
    {
        m_entries[row] = entry;
        emit dataChanged(index(row), index(row));
        return;
    }
    beginInsertRows({}, m_entries.size(), m_entries.size());
    m_entries.append(entry);
    endInsertRows();
}

void DeviceModel::remove(const QString &id)
{
    const int row = indexOf(id);
    if (row < 0)
    {
        return;
    }
    beginRemoveRows({}, row, row);
    m_entries.removeAt(row);
    endRemoveRows();
}

void DeviceModel::clear()
{
    beginResetModel();
    m_entries.clear();
    endResetModel();
}
