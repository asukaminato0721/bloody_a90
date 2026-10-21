// Private test recipient. QPlainTextEdit itself implements the zoom action.
#include <QApplication>
#include <QFont>
#include <QPlainTextEdit>
#include <QTextDocument>
#include <QTimer>
#include <QWheelEvent>
#include <fstream>
#include <iostream>

class Editor final : public QPlainTextEdit {
public:
    explicit Editor(const char *path) : log(path) {}
protected:
    void wheelEvent(QWheelEvent *event) override {
        const auto before = document()->defaultFont().pointSizeF();
        QPlainTextEdit::wheelEvent(event);
        const auto after = document()->defaultFont().pointSizeF();
        std::ofstream output(log, std::ios::app);
        output << event->angleDelta().y() << ' '
               << bool(event->modifiers() & Qt::ControlModifier) << ' '
               << before << ' ' << after << '\n';
    }
private:
    const char *log;
};

int main(int argc, char **argv) {
    if (argc != 2) return 2;
    QApplication application(argc, argv);
    Editor editor(argv[1]);
    editor.setWindowTitle(QStringLiteral("A90 isolated zoom"));
    editor.resize(400, 300);
    QFont font = editor.font();
    font.setPointSize(12);
    editor.setFont(font);
    editor.setPlainText(QStringLiteral("A90 zoom integration\nSecond line\nThird line"));
    editor.setReadOnly(true);
    editor.show();
    editor.setFocus();
    QTimer::singleShot(0, [&editor] { std::cout << "ready " << editor.winId() << std::endl; });
    QTimer::singleShot(10000, &application, &QApplication::quit);
    return application.exec();
}
