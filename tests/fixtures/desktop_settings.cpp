#include <QApplication>
#include <QStyleHints>
#include <iostream>

int main(int argc, char **argv) {
    QApplication app(argc, argv);
    std::cout << app.styleHints()->mouseDoubleClickInterval() << " "
              << app.styleHints()->wheelScrollLines() << std::endl;
}
