#pragma once

#include <KJob>

/// Progress of one transfer, shown by Plasma (notification area and job tracker).
/// Driven from outside: it never starts anything itself.
class TransferJob : public KJob
{
    Q_OBJECT

public:
    TransferJob(const QString &id, bool outbound, const QString &device, QObject *parent = nullptr);

    void start() override;
    void setProgress(qulonglong processed, qulonglong total);
    /// Ends the job; a failed job is reported as interrupted.
    /// `quietly` closes the job without a result notification.
    void finish(bool success, bool quietly = false);
    void setSpeed(qulonglong bytesPerSecond);
    void setSummary(const QString &summary) { m_summary = summary; }
    /// While the connection is being set up the job only says so and cannot be cancelled.
    void setConnecting(bool connecting);

Q_SIGNALS:
    void cancelRequested(const QString &id);

protected:
    bool doKill() override;

private:
    QString m_id;
    QString m_device;
    QString m_summary;
    bool m_outbound;
    bool m_done = false;
    bool m_connecting = false;

    void emitDescription();
};
