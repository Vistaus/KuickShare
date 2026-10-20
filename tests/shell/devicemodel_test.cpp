#include <QtTest>

#include "devicemodel.h"

class DeviceModelTest : public QObject
{
    Q_OBJECT

private Q_SLOTS:
    void keepsTheMediumAndNoAddressForBluetooth()
    {
        DeviceModel model;
        model.upsert(QStringLiteral("ble:G483"), QStringLiteral("Pixel"), QStringLiteral("Phone"), QString(), QString(), QStringLiteral("ble"));
        model.upsert(QStringLiteral("10.0.0.2:4000"), QStringLiteral("Desk"), QStringLiteral("Laptop"), QStringLiteral("10.0.0.2:4000"), QStringLiteral("trusted"), QStringLiteral("lan"));
        QCOMPARE(model.rowCount(), 2);
        QCOMPARE(model.data(model.index(0), DeviceModel::MediumRole).toString(), QStringLiteral("ble"));
        QVERIFY(model.data(model.index(0), DeviceModel::AddressRole).toString().isEmpty());
        QCOMPARE(model.data(model.index(1), DeviceModel::MediumRole).toString(), QStringLiteral("lan"));
    }

    void upsertReplacesAndRemoveDrops()
    {
        DeviceModel model;
        const QString id = QStringLiteral("ble:G483");
        model.upsert(id, QStringLiteral("Pixel"), QString(), QString(), QString(), QStringLiteral("ble"));
        model.upsert(id, QStringLiteral("Pixel 7"), QString(), QString(), QString(), QStringLiteral("ble"));
        QCOMPARE(model.rowCount(), 1);
        QCOMPARE(model.data(model.index(0), DeviceModel::NameRole).toString(), QStringLiteral("Pixel 7"));
        QCOMPARE(model.nameOf(id), QStringLiteral("Pixel 7"));
        model.remove(id);
        QCOMPARE(model.rowCount(), 0);
        QVERIFY(model.nameOf(id).isEmpty());
    }

    void aPhoneOnTheLanAndOverBluetoothIsListedOnce()
    {
        DeviceModel model;
        const QString name = QStringLiteral("Pixel");
        model.upsert(QStringLiteral("ble:G483"), name, QString(), QString(), QString(), QStringLiteral("ble"));
        QCOMPARE(model.rowCount(), 1);
        // The LAN entry arrives later: the Bluetooth one steps aside.
        model.upsert(QStringLiteral("10.0.0.5:4000"), name, QString(), QStringLiteral("10.0.0.5:4000"), QString(), QStringLiteral("lan"));
        QCOMPARE(model.rowCount(), 1);
        QCOMPARE(model.data(model.index(0), DeviceModel::MediumRole).toString(), QStringLiteral("lan"));
        // The Bluetooth entry is still known by name, and a repeated sighting keeps it hidden.
        QCOMPARE(model.nameOf(QStringLiteral("ble:G483")), name);
        model.upsert(QStringLiteral("ble:G483"), name, QString(), QString(), QString(), QStringLiteral("ble"));
        QCOMPARE(model.rowCount(), 1);
        // The phone leaves the LAN: the Bluetooth entry comes back.
        model.remove(QStringLiteral("10.0.0.5:4000"));
        QCOMPARE(model.rowCount(), 1);
        QCOMPARE(model.data(model.index(0), DeviceModel::MediumRole).toString(), QStringLiteral("ble"));
    }

    void twoDifferentPhonesStayTwoEntries()
    {
        DeviceModel model;
        model.upsert(QStringLiteral("ble:G483"), QStringLiteral("Pixel"), QString(), QString(), QString(), QStringLiteral("ble"));
        model.upsert(QStringLiteral("10.0.0.5:4000"), QStringLiteral("Desk"), QString(), QStringLiteral("10.0.0.5:4000"), QString(), QStringLiteral("lan"));
        QCOMPARE(model.rowCount(), 2);
    }
};

QTEST_GUILESS_MAIN(DeviceModelTest)
#include "devicemodel_test.moc"
