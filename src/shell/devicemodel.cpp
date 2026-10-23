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
    case MediumRole:
        return entry.medium;
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
        {MediumRole, "medium"},
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

bool DeviceModel::lanEntryNamed(const QString &name) const
{
    for (const Entry &entry : m_entries)
    {
        if (entry.medium != QLatin1String("ble") && entry.name == name)
        {
            return true;
        }
    }
    return false;
}

void DeviceModel::promoteHidden(const QString &name)
{
    if (lanEntryNamed(name))
    {
        return;
    }
    for (int i = 0; i < m_hidden.size(); ++i)
    {
        if (m_hidden[i].name == name)
        {
            beginInsertRows({}, m_entries.size(), m_entries.size());
            m_entries.append(m_hidden.takeAt(i));
            endInsertRows();
            return;
        }
    }
}

void DeviceModel::upsert(const QString &id, const QString &name, const QString &type, const QString &address, const QString &trust, const QString &medium)
{
    const Entry entry{id, name, type, address, trust, medium};
    const bool ble = medium == QLatin1String("ble");
    // A device seen again under a new name or medium: forget the old entries of this id first.
    for (int i = 0; i < m_hidden.size(); ++i)
    {
        if (m_hidden[i].id == id)
        {
            m_hidden.removeAt(i);
            break;
        }
    }
    if (ble && lanEntryNamed(name))
    {
        remove(id);
        m_hidden.append(entry);
        return;
    }
    if (const int row = indexOf(id); row >= 0)
    {
        const QString oldName = m_entries[row].name;
        m_entries[row] = entry;
        emit dataChanged(index(row), index(row));
        if (oldName != name)
        {
            promoteHidden(oldName);
        }
    }
    else
    {
        beginInsertRows({}, m_entries.size(), m_entries.size());
        m_entries.append(entry);
        endInsertRows();
    }
    if (!ble)
    {
        // The Bluetooth entry of the same phone steps aside.
        for (int row = 0; row < m_entries.size(); ++row)
        {
            if (m_entries[row].medium == QLatin1String("ble") && m_entries[row].name == name)
            {
                m_hidden.append(m_entries[row]);
                beginRemoveRows({}, row, row);
                m_entries.removeAt(row);
                endRemoveRows();
                break;
            }
        }
    }
}

void DeviceModel::remove(const QString &id)
{
    for (int i = 0; i < m_hidden.size(); ++i)
    {
        if (m_hidden[i].id == id)
        {
            m_hidden.removeAt(i);
            return;
        }
    }
    const int row = indexOf(id);
    if (row < 0)
    {
        return;
    }
    const QString name = m_entries[row].name;
    beginRemoveRows({}, row, row);
    m_entries.removeAt(row);
    endRemoveRows();
    promoteHidden(name);
}

void DeviceModel::clear()
{
    beginResetModel();
    m_entries.clear();
    m_hidden.clear();
    endResetModel();
}

QString DeviceModel::nameOf(const QString &id) const
{
    if (const int row = indexOf(id); row >= 0)
    {
        return m_entries.at(row).name;
    }
    for (const Entry &entry : m_hidden)
    {
        if (entry.id == id)
        {
            return entry.name;
        }
    }
    return {};
}
