#include "autostart.h"
#include "execquote.h"

#include <QCoreApplication>
#include <QDir>
#include <QFile>
#include <QSaveFile>
#include <QStandardPaths>

namespace
{
    QString entryPath()
    {
        return QStandardPaths::writableLocation(QStandardPaths::GenericConfigLocation) + QStringLiteral("/autostart/io.github.the_beper.KuickShare.desktop");
    }
}

bool Autostart::isEnabled()
{
    return QFile::exists(entryPath());
}

bool Autostart::setEnabled(bool enabled)
{
    const QString path = entryPath();
    if (!enabled)
    {
        return !QFile::exists(path) || QFile::remove(path);
    }

    if (!QDir().mkpath(QFileInfo(path).absolutePath()))
    {
        return false;
    }
    QSaveFile file(path);
    if (!file.open(QIODevice::WriteOnly | QIODevice::Text))
    {
        return false;
    }
    // The Exec path is the running binary, so re-enable after moving the installation.
    const QString exec = QCoreApplication::applicationFilePath();
    const QString content = QStringLiteral(
                                "[Desktop Entry]\n"
                                "Type=Application\n"
                                "Name=KuickShare\n"
                                "Comment=Quick Share client for KDE\n"
                                "Exec=%1 --background\n"
                                "Icon=io.github.the_beper.KuickShare\n"
                                "Terminal=false\n"
                                "X-KDE-autostart-after=panel\n")
                                .arg(quoteForExec(exec));
    file.write(content.toUtf8());
    return file.commit();
}
