#include <QTemporaryDir>
#include <QtTest>

#include "launcher.h"

class LauncherTest : public QObject
{
    Q_OBJECT

private Q_SLOTS:
    void outsideFlatpakTheExecIsTheQuotedProgram()
    {
        QCOMPARE(Launcher::hostExec(false, QStringLiteral("\"/opt/kuickshare\""), QStringLiteral("%F")), QStringLiteral("\"/opt/kuickshare\" %F"));
    }

    void insideFlatpakTheExecGoesThroughFlatpakRun()
    {
        QCOMPARE(Launcher::hostExec(true, QStringLiteral("\"/app/bin/kuickshare\""), QStringLiteral("@@ %F @@")),
                 QStringLiteral("flatpak run --file-forwarding io.github.the_beper.KuickShare @@ %F @@"));
        QCOMPARE(Launcher::hostExec(true, QStringLiteral("\"/app/bin/kuickshare\""), QStringLiteral("--background")),
                 QStringLiteral("flatpak run --file-forwarding io.github.the_beper.KuickShare --background"));
    }

    void insideAnAppImageTheExecIsTheAppImageFile()
    {
        QTemporaryDir dir;
        QVERIFY(dir.isValid());
        const QString image = dir.path() + QStringLiteral("/Kuick $Share.AppImage");
        QVERIFY(QFile(image).open(QIODevice::WriteOnly));
        qputenv("APPIMAGE", image.toUtf8());
        // The test binary plays the program inside the AppImage.
        qputenv("APPDIR", QCoreApplication::applicationDirPath().toUtf8());
        QVERIFY(Launcher::inAppImage());
        QCOMPARE(Launcher::hostExec(false, QStringLiteral("\"/tmp/.mount_x/usr/bin/kuickshare\""), QStringLiteral("%F")),
                 QStringLiteral("\"") + dir.path() + QStringLiteral("/Kuick \\$Share.AppImage\" %F"));
        QCOMPARE(Launcher::hostExec(false, QStringLiteral("\"/tmp/.mount_x/usr/bin/kuickshare\""), QStringLiteral("--background")),
                 QStringLiteral("\"") + dir.path() + QStringLiteral("/Kuick \\$Share.AppImage\" --background"));
        // The Flatpak launcher wins.
        QCOMPARE(Launcher::hostExec(true, QStringLiteral("\"x\""), QStringLiteral("%F")), QStringLiteral("flatpak run --file-forwarding io.github.the_beper.KuickShare %F"));
        qunsetenv("APPIMAGE");
    }

    void aMissingOrEmptyAppImageIsNotAnAppImage()
    {
        qputenv("APPIMAGE", "/nonexistent/KuickShare.AppImage");
        QVERIFY(!Launcher::inAppImage());
        QCOMPARE(Launcher::hostExec(false, QStringLiteral("\"/opt/k\""), QStringLiteral("%F")), QStringLiteral("\"/opt/k\" %F"));
        qputenv("APPIMAGE", "");
        QVERIFY(!Launcher::inAppImage());
        qunsetenv("APPIMAGE");
    }

    void aProgramOutsideAppDirIsNotTheAppImage()
    {
        // A host installation started from a terminal that inherited APPIMAGE.
        QTemporaryDir dir;
        QVERIFY(dir.isValid());
        const QString image = dir.path() + QStringLiteral("/KuickShare.AppImage");
        QVERIFY(QFile(image).open(QIODevice::WriteOnly));
        qputenv("APPIMAGE", image.toUtf8());
        qputenv("APPDIR", "/tmp/.mount_elsewhere");
        QVERIFY(!Launcher::inAppImage());
        qunsetenv("APPDIR");
        QVERIFY(!Launcher::inAppImage());
        qunsetenv("APPIMAGE");
    }

    void hostHomeIsFoundFromTheGrantedMount()
    {
        const QString info = QStringLiteral(
            "30 25 0:27 /home/me/.local/share/kio/servicemenus /srv/my\\040data/kio/servicemenus rw - ext4 /dev/sda rw\n"
            "31 25 0:27 /x /home/me/.var/app/io.github.the_beper.KuickShare/data/kio/servicemenus rw - ext4 /dev/sda rw\n");
        QCOMPARE(Launcher::hostHomeFromMountinfo(info, QStringLiteral("/kio/servicemenus"), QStringLiteral("/home/me/.var/app/io.github.the_beper.KuickShare/data")),
                 QStringLiteral("/srv/my data"));
    }

    void theSandboxOwnMountIsNotTheHostPath()
    {
        const QString info = QStringLiteral("31 25 0:27 /x /home/me/.var/app/id/config/autostart rw - ext4 /dev/sda rw\n");
        QVERIFY(Launcher::hostHomeFromMountinfo(info, QStringLiteral("/autostart"), QStringLiteral("/home/me/.var/app/id/config")).isEmpty());
    }

    void noMatchingMountGivesNothing()
    {
        QVERIFY(Launcher::hostHomeFromMountinfo(QStringLiteral("1 2 0:1 / / rw - ext4 /dev/sda rw\n"), QStringLiteral("/autostart"), QStringLiteral("/x")).isEmpty());
        QVERIFY(Launcher::hostHomeFromMountinfo({}, QStringLiteral("/autostart"), QStringLiteral("/x")).isEmpty());
    }

    void onlySharedFoldersAreSeenAsSharedWithTheHost()
    {
        const QString info = QStringLiteral(
            "1 0 0:1 / / rw - tmpfs tmpfs rw\n"
            "2 1 0:2 /d /home/me/Downloads rw - ext4 /dev/sda rw\n"
            "3 1 0:3 / /tmp rw - tmpfs tmpfs rw\n"
            "4 1 0:4 / /run/user/1000/doc rw - fuse.portal portal rw\n"
            "5 1 0:5 / /run/media/me/usb rw - vfat /dev/sdb rw\n"
            "6 1 0:6 / /data/dl rw - ext4 /dev/sdc rw\n"
            "7 1 0:7 / /run/user/1000/docker rw - tmpfs tmpfs rw\n");
        QVERIFY(Launcher::isSharedWithHost(info, QStringLiteral("/home/me/Downloads")));
        QVERIFY(Launcher::isSharedWithHost(info, QStringLiteral("/home/me/Downloads/KuickShare/")));
        QVERIFY(Launcher::isSharedWithHost(info, QStringLiteral("/run/user/1000/doc/abc/Folder")));
        QVERIFY(Launcher::isSharedWithHost(info, QStringLiteral("/run/media/me/usb/x")));
        QVERIFY(Launcher::isSharedWithHost(info, QStringLiteral("/data/dl")));
        QVERIFY(!Launcher::isSharedWithHost(info, QStringLiteral("/home/me/Documents")));
        QVERIFY(!Launcher::isSharedWithHost(info, QStringLiteral("/home/me/DownloadsX")));
        QVERIFY(!Launcher::isSharedWithHost(info, QStringLiteral("/tmp/x")));
        QVERIFY(!Launcher::isSharedWithHost(info, QStringLiteral("/run/user/1000/docker/x")));
    }

    void aGrantOfTheWholeHomeOrHostSharesEverythingBelow()
    {
        const QString home = QStringLiteral("1 0 0:1 / / rw - tmpfs tmpfs rw\n2 1 0:2 / /home/me rw - ext4 /dev/sda rw\n");
        QVERIFY(Launcher::isSharedWithHost(home, QStringLiteral("/home/me/Documents/x")));
        const QString host = QStringLiteral("1 0 0:1 / / rw - tmpfs tmpfs rw\n2 1 0:2 / /home rw - ext4 /dev/sda rw\n3 1 0:3 / /srv rw - ext4 /dev/sda rw\n");
        QVERIFY(Launcher::isSharedWithHost(host, QStringLiteral("/srv/files")));
    }

    void mountPointsAreUnescaped()
    {
        QCOMPARE(Launcher::mountPointOf(QStringLiteral("1 0 0:1 / /a\\040b\\134c rw - ext4 /dev/sda rw")), QStringLiteral("/a b\\c"));
    }

    void outsideFlatpakHostHomeIsTheStandardLocation()
    {
        QCOMPARE(Launcher::hostHome(false, QStandardPaths::GenericDataLocation), QStandardPaths::writableLocation(QStandardPaths::GenericDataLocation));
    }
};

QTEST_GUILESS_MAIN(LauncherTest)
#include "launcher_test.moc"
