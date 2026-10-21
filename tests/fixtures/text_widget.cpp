// Private recipient. Only real key events change the editor text.
#include <QApplication>
#include <QPlainTextEdit>
#include <QSaveFile>
#include <QTimer>
#include <iostream>

int main(int argc, char **argv) {
    if (argc != 2) return 2;
    QApplication application(argc, argv);
    QPlainTextEdit editor;
    editor.setWindowTitle(QStringLiteral("A90 isolated text recipient"));
    editor.resize(500, 350);
    editor.setTabChangesFocus(false);
    QObject::connect(&editor, &QPlainTextEdit::textChanged, [&] {
        QSaveFile file(QString::fromLocal8Bit(argv[1]));
        if (!file.open(QIODevice::WriteOnly)) application.exit(3);
        const auto text = editor.toPlainText().toUtf8();
        if (file.write(text) != text.size() || !file.commit()) application.exit(4);
    });
    editor.show();
    editor.setFocus();
    QTimer ready;
    ready.setInterval(10);
    QObject::connect(&ready, &QTimer::timeout, [&] {
        if (editor.isActiveWindow() && editor.hasFocus()) {
            ready.stop();
            std::cout << "ready" << std::endl;
        }
    });
    ready.start();
    QTimer::singleShot(15000, &application, &QApplication::quit);
    return application.exec();
}
