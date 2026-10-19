#include <csignal>

#include <QApplication>
#include <QCommandLineParser>
#include <QDir>
#include <QFileInfo>
#include <QIcon>
#include <QMenu>
#include <QQmlApplicationEngine>
#include <QQuickStyle>
#include <QQuickWindow>

#include <KAboutData>
#include <KDBusService>
#include <KLocalizedQmlContext>
#include <KLocalizedString>
#include <KSignalHandler>
#include <KStatusNotifierItem>

#include "corebridge.h"
#include "integration.h"
#include "dndwatcher.h"
#include "transferwatcher.h"

int main(int argc, char *argv[])
{
    QApplication app(argc, argv);
    QApplication::setQuitOnLastWindowClosed(false);
    if (qEnvironmentVariableIsEmpty("QT_QUICK_CONTROLS_STYLE"))
    {
        QQuickStyle::setStyle(QStringLiteral("org.kde.desktop"));
    }

    KLocalizedString::setApplicationDomain("kuickshare");
    KAboutData about(QStringLiteral("kuickshare"),
                     QStringLiteral("KuickShare"),
                     QStringLiteral(KUICKSHARE_VERSION),
                     i18n("Quick Share client for KDE"),
                     KAboutLicense::GPL_V3);
    about.setDesktopFileName(QStringLiteral("io.github.the_beper.KuickShare"));
    KAboutData::setApplicationData(about);
    // The app icons are installed with the application; fall back to a stock icon when running from a build tree.
    const QString appIcon = QIcon::hasThemeIcon(QStringLiteral("io.github.the_beper.KuickShare")) ? QStringLiteral("io.github.the_beper.KuickShare") : QStringLiteral("emblem-shared");
    const QString trayIcon = QIcon::hasThemeIcon(QStringLiteral("io.github.the_beper.KuickShare-symbolic")) ? QStringLiteral("io.github.the_beper.KuickShare-symbolic") : appIcon;
    QApplication::setWindowIcon(QIcon::fromTheme(appIcon));

    const QCommandLineOption background(QStringLiteral("background"), i18n("Start hidden in the system tray"));
    const QCommandLineOption page(QStringList{QStringLiteral("page")}, i18n("Open the given page: send, transfers, settings or help"), QStringLiteral("page"));
    auto setupParser = [&](QCommandLineParser &p)
    {
        about.setupCommandLine(&p);
        p.addOption(background);
        p.addOption(page);
        p.addPositionalArgument(QStringLiteral("files"), i18n("Files to send"), QStringLiteral("[files...]"));
    };
    QCommandLineParser parser;
    setupParser(parser);
    parser.process(app);
    about.processCommandLine(&parser);

    KSignalHandler::self()->watchSignal(SIGTERM);
    KSignalHandler::self()->watchSignal(SIGINT);
    QObject::connect(KSignalHandler::self(), &KSignalHandler::signalReceived, &app, &QApplication::quit);

    KDBusService service(KDBusService::Unique);

    CoreBridge core;
    TransferWatcher transfers(&core);
    DndWatcher dnd(&core);

    QQmlApplicationEngine engine;
    KLocalization::setupLocalizedContext(&engine);
    QVariantMap initial{{QStringLiteral("core"), QVariant::fromValue(&core)}};
    if (parser.isSet(page))
    {
        initial.insert(QStringLiteral("startPage"), parser.value(page));
    }
    initial.insert(QStringLiteral("askIntegration"), Integration::shouldAsk(parser.isSet(background)));
    engine.setInitialProperties(initial);
    engine.loadFromModule(QStringLiteral("io.github.the_beper.KuickShare"), QStringLiteral("Main"));
    if (engine.rootObjects().isEmpty())
    {
        return 1;
    }
    auto *window = qobject_cast<QQuickWindow *>(engine.rootObjects().constFirst());

    auto showWindow = [window]
    {
        window->show();
        window->raise();
        window->requestActivate();
    };
    auto queueFiles = [window](const QStringList &paths, const QString &workingDirectory)
    {
        QStringList files;
        for (const QString &path : paths)
        {
            const QString absolute = QDir(workingDirectory).absoluteFilePath(path);
            if (QFileInfo(absolute).isFile())
            {
                files << absolute;
            }
        }
        if (!files.isEmpty())
        {
            QMetaObject::invokeMethod(window, "queueFiles", Q_ARG(QVariant, QVariant(files)));
        }
    };
    QObject::connect(&service, &KDBusService::activateRequested, window, [=](const QStringList &arguments, const QString &workingDirectory)
                     {
        QCommandLineParser other;
        setupParser(other);
        other.parse(arguments);
        queueFiles(other.positionalArguments(), workingDirectory);
        showWindow(); });
    queueFiles(parser.positionalArguments(), QDir::currentPath());
    if (!parser.isSet(background))
    {
        showWindow();
    }

    KStatusNotifierItem tray;
    tray.setCategory(KStatusNotifierItem::ApplicationStatus);
    tray.setIconByName(trayIcon);
    tray.setTitle(QStringLiteral("KuickShare"));
    auto applyTray = [&tray, &core]
    {
        tray.setStatus(core.trayIcon() ? KStatusNotifierItem::Active : KStatusNotifierItem::Passive);
    };
    applyTray();
    QObject::connect(&core, &CoreBridge::trayIconChanged, &tray, applyTray);
    tray.setStandardActionsEnabled(false);
    QObject::connect(&tray, &KStatusNotifierItem::activateRequested, window, showWindow);

    auto inWindow = [window, showWindow](const char *function)
    {
        return [window, showWindow, function]
        {
            showWindow();
            QMetaObject::invokeMethod(window, function);
        };
    };

    auto *menu = new QMenu();
    menu->addAction(QIcon::fromTheme(QStringLiteral("window-new")), i18n("Open KuickShare"), showWindow);
    menu->addSeparator();
    menu->addAction(QIcon::fromTheme(QStringLiteral("document-send")), i18n("Send files…"), inWindow("openFiles"));
    menu->addAction(QIcon::fromTheme(QStringLiteral("edit-paste")), i18n("Send clipboard"), inWindow("pasteClipboard"));
    auto *visible = menu->addAction(QIcon::fromTheme(QStringLiteral("visibility")), i18n("Visible to nearby devices"));
    visible->setCheckable(true);
    visible->setChecked(core.visible());
    QObject::connect(visible, &QAction::triggered, &core, &CoreBridge::setVisible);
    QObject::connect(&core, &CoreBridge::visibleChanged, visible, [visible, &core]
                     { visible->setChecked(core.visible()); });
    menu->addSeparator();
    menu->addAction(QIcon::fromTheme(QStringLiteral("configure")), i18n("Preferences"), inWindow("showSettings"));
    menu->addAction(QIcon::fromTheme(QStringLiteral("application-exit")), i18n("Quit"), inWindow("requestQuit"));
    tray.setContextMenu(menu);

    return app.exec();
}
