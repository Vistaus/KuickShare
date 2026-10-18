#pragma once

#include <QString>

/// A program path as one quoted argument of a Desktop Entry `Exec` line, with the characters that
/// the specification wants escaped: inside double quotes `"`, `` ` ``, `$` and `\` get a backslash
/// (and a backslash is itself an escape in the file, so it is doubled once more), and `%` is `%%`
/// so it is not taken for a field code.
inline QString quoteForExec(const QString &path)
{
    QString quoted;
    quoted.reserve(path.size() + 2);
    quoted += QLatin1Char('"');
    for (const QChar c : path) {
        if (c == QLatin1Char('\\')) {
            quoted += QStringLiteral("\\\\\\\\");
        } else if (c == QLatin1Char('"') || c == QLatin1Char('`') || c == QLatin1Char('$')) {
            quoted += QLatin1Char('\\');
            quoted += c;
        } else if (c == QLatin1Char('%')) {
            quoted += QStringLiteral("%%");
        } else {
            quoted += c;
        }
    }
    quoted += QLatin1Char('"');
    return quoted;
}
