#include "dolphinmenu.h"
#include "execquote.h"

#include <QCoreApplication>
#include <QDir>
#include <QFile>
#include <QFileInfo>
#include <QSaveFile>
#include <QStandardPaths>

namespace
{
    QString entryPath()
    {
        return QStandardPaths::writableLocation(QStandardPaths::GenericDataLocation) + QStringLiteral("/kio/servicemenus/kuickshare-send.desktop");
    }
}

bool DolphinMenu::exists()
{
    return QFile::exists(entryPath());
}

bool DolphinMenu::setEnabled(bool enabled)
{
    const QString path = entryPath();
    if (!enabled)
    {
        return !QFile::exists(path) || QFile::remove(path);
    }

    // The entry is the one the installer uses (data/kuickshare-send.desktop), with the running binary as Exec.
    QFile source(QStringLiteral(":/data/kuickshare-send.desktop"));
    if (!source.open(QIODevice::ReadOnly | QIODevice::Text) || !QDir().mkpath(QFileInfo(path).absolutePath()))
    {
        return false;
    }
    QByteArray content = source.readAll();
    content.replace("Exec=kuickshare %F", QByteArrayLiteral("Exec=") + quoteForExec(QCoreApplication::applicationFilePath()).toUtf8() + QByteArrayLiteral(" %F"));

    QSaveFile file(path);
    if (!file.open(QIODevice::WriteOnly | QIODevice::Text))
    {
        return false;
    }
    file.write(content);
    // Dolphin ignores service menus that are not executable.
    return file.commit() && QFile::setPermissions(path, QFile::permissions(path) | QFileDevice::ExeOwner | QFileDevice::ExeGroup | QFileDevice::ExeOther);
}
