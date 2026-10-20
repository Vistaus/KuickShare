#include "transferjob.h"

#include <utility>

#include <KLocalizedString>

TransferJob::TransferJob(const QString &id, bool outbound, const QString &device, QObject *parent)
    : KJob(parent), m_id(id), m_device(device), m_outbound(outbound)
{
    setCapabilities(KJob::Killable);
}

void TransferJob::start()
{
    emitDescription();
}

void TransferJob::setConnecting(bool connecting)
{
    if (m_connecting == connecting)
    {
        return;
    }
    m_connecting = connecting;
    setCapabilities(connecting ? KJob::NoCapabilities : KJob::Killable);
    if (!connecting)
    {
        emitDescription();
    }
}

void TransferJob::emitDescription()
{
    Q_EMIT description(this,
                       m_connecting ? i18n("Connecting…") : m_outbound ? i18n("Sending files")
                                                                        : i18n("Receiving files"),
                       qMakePair(m_outbound ? i18n("To") : i18n("From"), m_device),
                       m_summary.isEmpty() ? QPair<QString, QString>() : qMakePair(i18n("Files"), m_summary));
}

void TransferJob::setProgress(qulonglong processed, qulonglong total)
{
    if (m_done)
    {
        return;
    }
    setTotalAmount(KJob::Bytes, total);
    setProcessedAmount(KJob::Bytes, processed);
}

void TransferJob::setSpeed(qulonglong bytesPerSecond)
{
    if (!m_done)
    {
        emitSpeed(bytesPerSecond);
    }
}

void TransferJob::finish(bool success, bool quietly)
{
    if (m_done)
    {
        return;
    }
    m_done = true;
    if (!success)
    {
        setError(KJob::UserDefinedError);
        setErrorText(i18n("The transfer was interrupted"));
    }
    if (quietly)
    {
        kill(KJob::Quietly);
    }
    else
    {
        emitResult();
    }
}

bool TransferJob::doKill()
{
    // Closing a finished job quietly must not cancel the transfer again.
    if (!std::exchange(m_done, true))
    {
        Q_EMIT cancelRequested(m_id);
    }
    return true;
}
