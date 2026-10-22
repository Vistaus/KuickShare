#include <QFile>
#include <QFileInfo>
#include <QTemporaryDir>
#include <QtTest>

#include "integration.h"

class IntegrationTest : public QObject
{
    Q_OBJECT

private Q_SLOTS:
    void init()
    {
        QVERIFY(m_dir.isValid());
        QVERIFY(m_image.isValid());
        const QString image = m_image.path() + QStringLiteral("/KuickShare.AppImage");
        QVERIFY(QFile(image).open(QIODevice::WriteOnly));
        // Scratch XDG folders and home, and a (fake) AppImage.
        qputenv("XDG_DATA_HOME", (m_dir.path() + QStringLiteral("/data")).toUtf8());
        qputenv("XDG_CONFIG_HOME", (m_dir.path() + QStringLiteral("/config")).toUtf8());
        qputenv("HOME", m_dir.path().toUtf8());
        qputenv("APPIMAGE", image.toUtf8());
        qputenv("XDG_CURRENT_DESKTOP", "KDE");
        // The test binary plays the program inside the AppImage.
        qputenv("APPDIR", QCoreApplication::applicationDirPath().toUtf8());
    }

    void cleanup()
    {
        QDir(m_dir.path()).removeRecursively();
        QDir().mkpath(m_dir.path());
    }

    void asksOnceInAFreshAppImage()
    {
        QVERIFY(Integration::shouldAsk(false));
        QVERIFY(Integration::markAsked());
        QVERIFY(!Integration::shouldAsk(false));
    }

    void neverAsksWhenStartedHiddenOrOutsideAnAppImage()
    {
        QVERIFY(!Integration::shouldAsk(true));
        qunsetenv("APPIMAGE");
        QVERIFY(!Integration::shouldAsk(false));
    }

    void neverAsksWithoutADesktop()
    {
        qunsetenv("XDG_CURRENT_DESKTOP");
        QVERIFY(!Integration::shouldAsk(false));
    }

    void neverAsksWhenAlreadyIntegrated()
    {
        QVERIFY(Integration::installMenuEntry());
        QVERIFY(!Integration::shouldAsk(false));
        QDir(m_dir.path()).removeRecursively();

        QDir().mkpath(m_dir.path() + QStringLiteral("/.local/bin"));
        QVERIFY(QFile(m_dir.path() + QStringLiteral("/.local/bin/kuickshare")).open(QIODevice::WriteOnly));
        QVERIFY(!Integration::shouldAsk(false));
        QDir(m_dir.path()).removeRecursively();

        QDir().mkpath(m_dir.path() + QStringLiteral("/data/kio/servicemenus"));
        QVERIFY(QFile(m_dir.path() + QStringLiteral("/data/kio/servicemenus/kuickshare-send.desktop")).open(QIODevice::WriteOnly));
        QVERIFY(!Integration::shouldAsk(false));
    }

    void theMenuEntryCanBeRemovedAgain()
    {
        QVERIFY(!Integration::menuEntryExists());
        QVERIFY(Integration::installMenuEntry());
        QVERIFY(Integration::menuEntryExists());
        QVERIFY(Integration::removeMenuEntry());
        QVERIFY(!Integration::menuEntryExists());
        QVERIFY(!QFileInfo::exists(m_dir.path() + QStringLiteral("/data/icons/hicolor/scalable/apps/io.github.the_beper.KuickShare.svg")));
        QVERIFY(Integration::removeMenuEntry());
    }

    void theMenuEntryPointsAtTheAppImageAndBringsItsIcons()
    {
        QVERIFY(Integration::installMenuEntry());
        QFile file(m_dir.path() + QStringLiteral("/data/applications/io.github.the_beper.KuickShare.desktop"));
        QVERIFY(file.open(QIODevice::ReadOnly | QIODevice::Text));
        const QByteArray content = file.readAll();
        QVERIFY(content.contains("Exec=\"" + m_image.path().toUtf8() + "/KuickShare.AppImage\" %U\n"));
        QVERIFY(!content.contains("Exec=kuickshare"));
        QVERIFY(QFileInfo::exists(m_dir.path() + QStringLiteral("/data/icons/hicolor/scalable/apps/io.github.the_beper.KuickShare.svg")));
        QVERIFY(QFileInfo::exists(m_dir.path() + QStringLiteral("/data/icons/hicolor/symbolic/apps/io.github.the_beper.KuickShare-symbolic.svg")));
    }

private:
    QTemporaryDir m_dir;
    QTemporaryDir m_image;
};

QTEST_GUILESS_MAIN(IntegrationTest)
#include "integration_test.moc"
