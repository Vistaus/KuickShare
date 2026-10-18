#include <QtTest>

#include "execquote.h"

class ExecQuoteTest : public QObject
{
    Q_OBJECT

private Q_SLOTS:
    void aPlainPathIsJustQuoted()
    {
        QCOMPARE(quoteForExec(QStringLiteral("/home/me/.local/share/kuickshare/bin/kuickshare")), QStringLiteral("\"/home/me/.local/share/kuickshare/bin/kuickshare\""));
        QCOMPARE(quoteForExec(QStringLiteral("/home/me/with space/kuickshare")), QStringLiteral("\"/home/me/with space/kuickshare\""));
    }

    void specialCharactersAreEscaped()
    {
        // Inside the quotes: " ` $ get a backslash, a backslash becomes four, % is doubled.
        QCOMPARE(quoteForExec(QStringLiteral("/a\"b")), QStringLiteral("\"/a\\\"b\""));
        QCOMPARE(quoteForExec(QStringLiteral("/a`b$c")), QStringLiteral("\"/a\\`b\\$c\""));
        QCOMPARE(quoteForExec(QStringLiteral("/a\\b")), QStringLiteral("\"/a\\\\\\\\b\""));
        QCOMPARE(quoteForExec(QStringLiteral("/100%/x")), QStringLiteral("\"/100%%/x\""));
    }
};

QTEST_APPLESS_MAIN(ExecQuoteTest)
#include "execquote_test.moc"
