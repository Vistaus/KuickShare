#include "transferwatcher.h"

#include <QClipboard>
#include <QDesktopServices>
#include <QGuiApplication>
#include <QRegularExpression>
#include <QStandardPaths>
#include <QUrl>

#include <KLocalizedString>

#include "corebridge.h"
#include "transferjob.h"

namespace
{
    bool isTransferring(const QString &state)
    {
        return state == QLatin1String("ReceivingFiles") || state == QLatin1String("SendingFiles");
    }

    QString shortened(const QString &text)
    {
        return text.size() > 200 ? text.left(200) + QStringLiteral("…") : text;
    }
}

TransferWatcher::TransferWatcher(CoreBridge *core, QObject *parent)
    : QObject(parent), m_core(core), m_tracker(this)
{
    m_clock.start();
    connect(core, &CoreBridge::transferChanged, this, &TransferWatcher::onTransferChanged);
}

void TransferWatcher::onTransferChanged(const QString &id)
{
    const QVariantMap info = m_core->transfers()->info(id);
    const QString state = info.value(QStringLiteral("state")).toString();
    const bool outbound = info.value(QStringLiteral("kind")).toString() == QLatin1String("outbound");
    const QString device = info.value(QStringLiteral("device")).toString();
    const QString preview = info.value(QStringLiteral("preview")).toString();
    const qulonglong total = info.value(QStringLiteral("total_bytes")).toULongLong();
    const qulonglong acked = info.value(QStringLiteral("ack_bytes")).toULongLong();

    Tracked &tracked = m_tracked[id];
    if (tracked.finished)
    {
        if (TransferModel::isFinal(state))
        {
            return;
        }
        tracked = Tracked(); // the same id starts a new transfer
    }

    // A paired computer announced this transfer: the core accepts it by itself.
    const bool autoAccepted = !info.value(QStringLiteral("trusted")).toString().isEmpty();
    const bool awaitingConsent = !outbound && state == QLatin1String("WaitingForUserConsent") && !autoAccepted;
    if (awaitingConsent)
    {
        if (!tracked.consent)
        {
            notifyConsent(id, device, info.value(QStringLiteral("pin")).toString(), tracked);
        }
    }
    else if (tracked.consent)
    {
        tracked.consent->close();
    }

    // Text and links are tiny: only real file transfers get a progress job.
    if (isTransferring(state))
    {
        tracked.started = true;
    }
    if (isTransferring(state) && total > 0 && preview.isEmpty())
    {
        if (!tracked.job)
        {
            auto *job = new TransferJob(id, outbound, device, this);
            connect(job, &TransferJob::cancelRequested, m_core, &CoreBridge::cancel);
            if (const int files = outbound ? m_core->outgoingFileCount(id) : 0; files > 0)
            {
                job->setSummary(i18np("%1 file", "%1 files", files));
            }
            m_tracker.registerJob(job);
            job->start();
            tracked.job = job;
        }
        tracked.job->setProgress(acked, total);

        // Instantaneous speed: bytes since the last sample, at most twice a second, lightly smoothed.
        const qint64 now = m_clock.elapsed();
        if (tracked.sampleMs == 0)
        {
            tracked.sampleMs = now;
            tracked.sampleBytes = acked;
        }
        else if (now - tracked.sampleMs >= 500)
        {
            const double instant = acked >= tracked.sampleBytes ? double(acked - tracked.sampleBytes) * 1000.0 / double(now - tracked.sampleMs) : 0.0;
            tracked.speed = tracked.speed == 0 ? instant : 0.5 * tracked.speed + 0.5 * instant;
            tracked.job->setSpeed(qulonglong(tracked.speed));
            tracked.sampleMs = now;
            tracked.sampleBytes = acked;
        }
    }

    if (!TransferModel::isFinal(state))
    {
        return;
    }
    tracked.finished = true;
    const bool success = state == QLatin1String("Finished");
    // A received file gets its own "Files received" notification: close the job without a second one.
    if (tracked.job)
    {
        tracked.job->finish(success, success && !outbound && preview.isEmpty());
    }

    const QString who = device.isEmpty() ? i18n("a nearby device") : device;
    if (success && !outbound)
    {
        notifyReceived(who, preview);
    }
    else if (outbound && !tracked.started && (state == QLatin1String("Disconnected") || state == QLatin1String("Rejected")))
    {
        // The library reports a refusal by the other device as "Disconnected"; before any data
        // moved that is the most likely reason (it can also be a full disk or a timeout).
        notifyProblem(QStringLiteral("transferFailed"), i18n("Transfer not accepted"), i18n("%1 declined the transfer or could not receive it", who.toHtmlEscaped()));
    }
    else if (state == QLatin1String("Disconnected"))
    {
        notifyProblem(QStringLiteral("transferFailed"), i18n("Transfer interrupted"), i18n("The connection with %1 was lost", who.toHtmlEscaped()));
    }
}

void TransferWatcher::notifyConsent(const QString &id, const QString &device, const QString &pin, Tracked &tracked)
{
    auto *notification = new KNotification(QStringLiteral("incomingTransfer"), KNotification::Persistent);
    notification->setIconName(QStringLiteral("io.github.the_beper.KuickShare"));
    notification->setTitle(i18n("Incoming transfer"));
    const QString who = device.isEmpty() ? i18n("A nearby device") : device.toHtmlEscaped();
    notification->setText(pin.isEmpty() ? i18n("%1 wants to share with you", who) : i18n("%1 wants to share with you\nPIN: %2", who, pin.toHtmlEscaped()));

    auto *accept = notification->addAction(i18n("Accept"));
    connect(accept, &KNotificationAction::activated, this, [this, id]
            { m_core->accept(id); });
    // Trusting a device is a feature that can be turned off in the settings. It needs the sender's
    // MAC address, which only exists for transfers over the network (id "ip:port"), not for Bluetooth
    // ones (id "ble-N"). The core reads the MAC from the IPv4 neighbour table, so an IPv6 id would
    // need the same check there.
    if (m_core->trustedDevicesEnabled() && id.contains(QLatin1Char(':')))
    {
        auto *trust = notification->addAction(i18n("Accept and trust"));
        connect(trust, &KNotificationAction::activated, this, [this, id]
                {
            m_core->allowDevice(id);
            m_core->accept(id); });
    }
    auto *decline = notification->addAction(i18n("Decline"));
    connect(decline, &KNotificationAction::activated, this, [this, id]
            { m_core->decline(id); });

    notification->sendEvent();
    tracked.consent = notification;
}

void TransferWatcher::notifyReceived(const QString &who, const QString &preview)
{
    auto *notification = new KNotification(QStringLiteral("transferFinished"));
    notification->setIconName(QStringLiteral("io.github.the_beper.KuickShare"));

    if (!preview.isEmpty())
    {
        notification->setTitle(i18n("Text received"));
        notification->setText(i18n("From %1:\n%2", who.toHtmlEscaped(), shortened(preview).toHtmlEscaped()));
        auto *copy = notification->addAction(i18n("Copy"));
        connect(copy, &KNotificationAction::activated, this, [preview]
                { QGuiApplication::clipboard()->setText(preview); });
        static const QRegularExpression link(QStringLiteral("^https?://\\S+$"), QRegularExpression::CaseInsensitiveOption);
        if (link.match(preview.trimmed()).hasMatch())
        {
            auto *open = notification->addAction(i18n("Open link"));
            connect(open, &KNotificationAction::activated, this, [preview]
                    { QDesktopServices::openUrl(QUrl(preview.trimmed())); });
        }
    }
    else
    {
        const QString dir = m_core->downloadDir().isEmpty() ? QStandardPaths::writableLocation(QStandardPaths::DownloadLocation) : m_core->downloadDir();
        notification->setTitle(i18n("Files received"));
        notification->setText(i18n("From %1", who.toHtmlEscaped()));
        auto *open = notification->addAction(i18n("Open folder"));
        connect(open, &KNotificationAction::activated, this, [dir]
                { QDesktopServices::openUrl(QUrl::fromLocalFile(dir)); });
    }
    notification->sendEvent();
}

void TransferWatcher::notifyProblem(const QString &eventId, const QString &title, const QString &text)
{
    auto *notification = new KNotification(eventId);
    notification->setIconName(QStringLiteral("dialog-warning"));
    notification->setTitle(title);
    notification->setText(text);
    notification->sendEvent();
}
