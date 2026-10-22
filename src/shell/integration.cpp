#include "integration.h"
#include "dolphinmenu.h"
#include "execquote.h"
#include "launcher.h"

#include <QCoreApplication>
#include <QDir>
#include <QFile>
#include <QFileInfo>
#include <QSaveFile>
#include <QStandardPaths>

namespace
{
    const QString appId = QStringLiteral("io.github.the_beper.KuickShare");

    QString dataDir()
    {
        return QStandardPaths::writableLocation(QStandardPaths::GenericDataLocation);
    }

    QString menuEntryPath()
    {
        return dataDir() + QStringLiteral("/applications/") + appId + QStringLiteral(".desktop");
    }

    QString markerPath()
    {
        return QStandardPaths::writableLocation(QStandardPaths::GenericConfigLocation) + QStringLiteral("/kuickshare/integration-asked");
    }

    bool copyResource(const QString &resource, const QString &path, const QByteArray &exec = {})
    {
        QFile source(resource);
        if (!source.open(QIODevice::ReadOnly) || !QDir().mkpath(QFileInfo(path).absolutePath()))
        {
            return false;
        }
        QByteArray content = source.readAll();
        if (!exec.isEmpty())
        {
            content.replace("Exec=kuickshare", QByteArrayLiteral("Exec=") + exec);
        }
        QSaveFile file(path);
        if (!file.open(QIODevice::WriteOnly))
        {
            return false;
        }
        file.write(content);
        return file.commit();
    }
}

bool Integration::shouldAsk(bool background)
{
    // No XDG_CURRENT_DESKTOP: a bare session (Xvfb, a container, a catalog test bot), with no menu to add an entry to.
    if (background || !Launcher::inAppImage() || qEnvironmentVariableIsEmpty("XDG_CURRENT_DESKTOP") || QFile::exists(markerPath()))
    {
        return false;
    }
    return !DolphinMenu::exists() && !QFile::exists(menuEntryPath()) && !QFile::exists(QDir::homePath() + QStringLiteral("/.local/bin/kuickshare"));
}

bool Integration::markAsked()
{
    const QString path = markerPath();
    if (!QDir().mkpath(QFileInfo(path).absolutePath()))
    {
        return false;
    }
    QFile file(path);
    return file.open(QIODevice::WriteOnly);
}

bool Integration::installMenuEntry()
{
    const QString exec = Launcher::hostExec(false, quoteForExec(QCoreApplication::applicationFilePath()), QStringLiteral("%U"));
    const QString icons = dataDir() + QStringLiteral("/icons/hicolor");
    return copyResource(QStringLiteral(":/data/icons/hicolor/scalable/apps/") + appId + QStringLiteral(".svg"), icons + QStringLiteral("/scalable/apps/") + appId + QStringLiteral(".svg")) && copyResource(QStringLiteral(":/data/icons/hicolor/symbolic/apps/") + appId + QStringLiteral("-symbolic.svg"), icons + QStringLiteral("/symbolic/apps/") + appId + QStringLiteral("-symbolic.svg")) && copyResource(QStringLiteral(":/data/") + appId + QStringLiteral(".desktop"), menuEntryPath(), exec.toUtf8());
}

bool Integration::menuEntryExists()
{
    return QFile::exists(menuEntryPath());
}

bool Integration::removeMenuEntry()
{
    const QString icons = dataDir() + QStringLiteral("/icons/hicolor");
    bool ok = true;
    for (const QString &path : {menuEntryPath(), icons + QStringLiteral("/scalable/apps/") + appId + QStringLiteral(".svg"), icons + QStringLiteral("/symbolic/apps/") + appId + QStringLiteral("-symbolic.svg")})
    {
        ok = (!QFile::exists(path) || QFile::remove(path)) && ok;
    }
    return ok;
}
