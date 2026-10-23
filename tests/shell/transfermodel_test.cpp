#include <QtTest>

#include "transfermodel.h"

class TransferModelTest : public QObject
{
    Q_OBJECT

private:
    static QVariantMap event(const QString &state, const QString &device = {})
    {
        return {{QStringLiteral("state"), state}, {QStringLiteral("device"), device}};
    }
    static QString stateAt(const TransferModel &model, int row)
    {
        return model.data(model.index(row), TransferModel::StateRole).toString();
    }

private Q_SLOTS:
    void updatesMergeAndKeepKnownFields()
    {
        TransferModel model;
        model.update(QStringLiteral("a"), event(QStringLiteral("SendingFiles"), QStringLiteral("Pixel")));
        model.update(QStringLiteral("a"), event(QStringLiteral("SendingFiles"))); // device now empty
        QCOMPARE(model.rowCount(), 1);
        QCOMPARE(model.info(QStringLiteral("a")).value(QStringLiteral("device")).toString(), QStringLiteral("Pixel"));
    }

    void finishedTransferIsRestartedAsNewRow()
    {
        TransferModel model;
        model.update(QStringLiteral("a"), event(QStringLiteral("Finished")));
        model.update(QStringLiteral("a"), event(QStringLiteral("SendingFiles"))); // same id, new transfer
        QCOMPARE(model.rowCount(), 2);
        QCOMPARE(stateAt(model, 0), QStringLiteral("SendingFiles"));
        QCOMPARE(stateAt(model, 1), QStringLiteral("Finished"));
        QCOMPARE(model.info(QStringLiteral("a")).value(QStringLiteral("state")).toString(), QStringLiteral("SendingFiles"));
    }

    void pendingSendIsTakenOverByTheFirstEvent()
    {
        TransferModel model;
        model.update(QStringLiteral("ble:X"), {{QStringLiteral("kind"), QStringLiteral("outbound")},
                                               {QStringLiteral("state"), QStringLiteral("Starting")},
                                               {QStringLiteral("device"), QStringLiteral("Pixel")}});
        model.update(QStringLiteral("ble:X"), event(QStringLiteral("SentIntroduction")));
        QCOMPARE(model.rowCount(), 1);
        QCOMPARE(stateAt(model, 0), QStringLiteral("SentIntroduction"));
        QCOMPARE(model.data(model.index(0), TransferModel::KindRole).toString(), QStringLiteral("outbound"));
        QCOMPARE(model.data(model.index(0), TransferModel::DeviceRole).toString(), QStringLiteral("Pixel"));
    }

    void refusedPendingSendEndsAsOneFinishedRow()
    {
        TransferModel model;
        model.update(QStringLiteral("a"), event(QStringLiteral("Starting"), QStringLiteral("Pixel")));
        model.update(QStringLiteral("a"), event(QStringLiteral("Disconnected")));
        model.update(QStringLiteral("a"), event(QStringLiteral("Disconnected"))); // the watchdog later
        QCOMPARE(model.rowCount(), 1);
        QVERIFY(TransferModel::isFinal(stateAt(model, 0)));
        model.update(QStringLiteral("a"), event(QStringLiteral("Starting"), QStringLiteral("Pixel"))); // a retry
        QCOMPARE(model.rowCount(), 2);
    }

    void anOlderSendFailureDoesNotEndANewerSend()
    {
        TransferModel model;
        const quint64 first = model.beginPending(QStringLiteral("a"), {{QStringLiteral("device"), QStringLiteral("Pixel")}});
        model.failPending(QStringLiteral("a"), first);
        QCOMPARE(stateAt(model, 0), QStringLiteral("Disconnected"));
        const quint64 second = model.beginPending(QStringLiteral("a"), {{QStringLiteral("device"), QStringLiteral("Pixel")}});
        QCOMPARE(model.rowCount(), 2);
        QVERIFY(second != first);
        model.failPending(QStringLiteral("a"), first); // the timer of the first send fires late
        QCOMPARE(stateAt(model, 0), QStringLiteral("Starting"));
        model.failPending(QStringLiteral("a"), second);
        QCOMPARE(stateAt(model, 0), QStringLiteral("Disconnected"));
    }

    void failurePendingIgnoresASendTheCoreHasTakenOver()
    {
        TransferModel model;
        const quint64 number = model.beginPending(QStringLiteral("a"), {});
        model.update(QStringLiteral("a"), event(QStringLiteral("SentIntroduction")));
        model.failPending(QStringLiteral("a"), number);
        QCOMPARE(stateAt(model, 0), QStringLiteral("SentIntroduction"));
    }

    void historyIsAppendedBelowLiveTransfers()
    {
        TransferModel model;
        model.addHistory(event(QStringLiteral("Finished"), QStringLiteral("old")));
        model.update(QStringLiteral("live"), event(QStringLiteral("SendingFiles"), QStringLiteral("now")));
        QCOMPARE(model.rowCount(), 2);
        QCOMPARE(stateAt(model, 0), QStringLiteral("SendingFiles"));
        QCOMPARE(stateAt(model, 1), QStringLiteral("Finished"));
    }

    void clearFinishedKeepsActiveTransfers()
    {
        TransferModel model;
        model.addHistory(event(QStringLiteral("Finished")));
        model.addHistory(event(QStringLiteral("Disconnected")));
        model.update(QStringLiteral("live"), event(QStringLiteral("ReceivingFiles")));
        model.clearFinished();
        QCOMPARE(model.rowCount(), 1);
        QCOMPARE(stateAt(model, 0), QStringLiteral("ReceivingFiles"));
    }

    void filesRoleSplitsOnNewlines()
    {
        TransferModel model;
        QVariantMap info = event(QStringLiteral("Finished"));
        info.insert(QStringLiteral("files"), QStringLiteral("a.mp4\nb.pdf"));
        model.addHistory(info);
        QCOMPARE(model.data(model.index(0), TransferModel::FilesRole).toStringList(), (QStringList{QStringLiteral("a.mp4"), QStringLiteral("b.pdf")}));
    }

    void onlySendingOrReceivingCountsAsActive()
    {
        TransferModel model;
        QVERIFY(!model.hasActive());
        model.update(QStringLiteral("a"), event(QStringLiteral("WaitingForUserConsent")));
        QVERIFY(!model.hasActive()); // waiting for the user is not a transfer in progress
        model.update(QStringLiteral("b"), event(QStringLiteral("ReceivingFiles")));
        QVERIFY(model.hasActive());
        model.update(QStringLiteral("b"), event(QStringLiteral("Finished")));
        QVERIFY(!model.hasActive());
        model.update(QStringLiteral("c"), event(QStringLiteral("SendingFiles")));
        QVERIFY(model.hasActive());
    }

    void removeDeletesOneRowAndIgnoresUnknownIds()
    {
        TransferModel model;
        model.update(QStringLiteral("a"), event(QStringLiteral("Finished"), QStringLiteral("Pixel")));
        model.update(QStringLiteral("b"), event(QStringLiteral("Finished"), QStringLiteral("Tab")));
        model.remove(QStringLiteral("nope"));
        QCOMPARE(model.rowCount(), 2);
        model.remove(QStringLiteral("a"));
        QCOMPARE(model.rowCount(), 1);
        QCOMPARE(model.data(model.index(0), TransferModel::DeviceRole).toString(), QStringLiteral("Tab"));
    }

    void secondFinalStateIsIgnored()
    {
        TransferModel model;
        model.update(QStringLiteral("a"), event(QStringLiteral("Finished")));
        model.update(QStringLiteral("a"), event(QStringLiteral("Disconnected")));
        QCOMPARE(model.rowCount(), 1);
        QCOMPARE(stateAt(model, 0), QStringLiteral("Finished"));
    }
};

QTEST_APPLESS_MAIN(TransferModelTest)
#include "transfermodel_test.moc"
