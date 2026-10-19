#pragma once

#include <QByteArray>
#include <QCoreApplication>
#include <QDir>
#include <QFile>
#include <QFileInfo>
#include <QStringList>
#include <QStandardPaths>
#include <QString>

#include <algorithm>

#include "execquote.h"

namespace Launcher
{
    inline QString flatpakId()
    {
        return QStringLiteral("io.github.the_beper.KuickShare");
    }

    inline bool inFlatpak()
    {
        return QFile::exists(QStringLiteral("/.flatpak-info"));
    }

    inline QString mountPointOf(const QString &line)
    {
        QString mountPoint = line.section(QLatin1Char(' '), 4, 4);
        mountPoint.replace(QStringLiteral("\\040"), QStringLiteral(" ")).replace(QStringLiteral("\\011"), QStringLiteral("\t")).replace(QStringLiteral("\\012"), QStringLiteral("\n")).replace(QStringLiteral("\\134"), QStringLiteral("\\"));
        return mountPoint;
    }

    inline QString hostHomeFromMountinfo(const QString &mountinfo, const QString &suffix, const QString &sandboxHome)
    {
        for (const QString &line : mountinfo.split(QLatin1Char('\n')))
        {
            const QString mountPoint = mountPointOf(line);
            if (mountPoint.endsWith(suffix) && !mountPoint.startsWith(sandboxHome + QLatin1Char('/')))
            {
                return mountPoint.chopped(suffix.size());
            }
        }
        return {};
    }

    inline QString hostHome(bool flatpak, QStandardPaths::StandardLocation location)
    {
        if (!flatpak)
        {
            return QStandardPaths::writableLocation(location);
        }
        const bool data = location == QStandardPaths::GenericDataLocation;
        QFile file(QStringLiteral("/proc/self/mountinfo"));
        if (file.open(QIODevice::ReadOnly | QIODevice::Text))
        {
            const QString found = hostHomeFromMountinfo(QString::fromUtf8(file.readAll()),
                                                        data ? QStringLiteral("/kio/servicemenus") : QStringLiteral("/autostart"),
                                                        QStandardPaths::writableLocation(location));
            if (!found.isEmpty())
            {
                return found;
            }
        }
        return QDir::homePath() + (data ? QStringLiteral("/.local/share") : QStringLiteral("/.config"));
    }

    inline QString appImagePath()
    {
        return QString::fromLocal8Bit(qgetenv("APPIMAGE"));
    }

    inline bool inAppImage()
    {
        const QString path = appImagePath();
        const QString appDir = QDir::cleanPath(QString::fromLocal8Bit(qgetenv("APPDIR")));
        return !path.isEmpty() && QFileInfo::exists(path) && !appDir.isEmpty() && appDir != QLatin1String(".") && QCoreApplication::applicationFilePath().startsWith(appDir + QLatin1Char('/'));
    }

    inline bool isSharedWithHost(const QString &mountinfo, const QString &path)
    {
        static const QStringList privateRoots{QStringLiteral("/tmp"), QStringLiteral("/run"), QStringLiteral("/var"), QStringLiteral("/usr"), QStringLiteral("/etc"),
                                              QStringLiteral("/app"), QStringLiteral("/dev"), QStringLiteral("/proc"), QStringLiteral("/sys"), QStringLiteral("/lib"),
                                              QStringLiteral("/lib64"), QStringLiteral("/bin"), QStringLiteral("/sbin")};
        auto under = [](const QString &child, const QString &root)
        { return child == root || child.startsWith(root + QLatin1Char('/')); };
        const QFileInfo info(path);
        const QString clean = info.exists() ? info.canonicalFilePath() : QDir::cleanPath(path);
        for (const QString &line : mountinfo.split(QLatin1Char('\n')))
        {
            const QString mountPoint = mountPointOf(line);
            if (mountPoint.isEmpty() || mountPoint == QLatin1String("/") || !under(clean, mountPoint))
            {
                continue;
            }
            const bool portal = mountPoint.startsWith(QStringLiteral("/run/user/")) && mountPoint.section(QLatin1Char('/'), 4, 4) == QLatin1String("doc");
            const bool media = mountPoint.startsWith(QStringLiteral("/run/media/"));
            const bool isPrivate = std::any_of(privateRoots.cbegin(), privateRoots.cend(), [&](const QString &root)
                                               { return under(mountPoint, root); });
            if (portal || media || !isPrivate)
            {
                return true;
            }
        }
        return false;
    }

    inline bool isSharedWithHost(const QString &path)
    {
        QFile file(QStringLiteral("/proc/self/mountinfo"));
        return file.open(QIODevice::ReadOnly | QIODevice::Text) && isSharedWithHost(QString::fromUtf8(file.readAll()), path);
    }

    inline QString hostExec(bool flatpak, const QString &quotedProgram, const QString &arguments)
    {
        if (flatpak)
        {
            return QStringLiteral("flatpak run --file-forwarding ") + flatpakId() + QLatin1Char(' ') + arguments;
        }
        if (inAppImage())
        {
            return quoteForExec(appImagePath()) + QLatin1Char(' ') + arguments;
        }
        return quotedProgram + QLatin1Char(' ') + arguments;
    }
}
