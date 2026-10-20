import QtQuick
import QtQuick.Window as QuickWindow
import org.kde.kwin
import org.kde.plasma.workspace.dbus 1.0 as DBus

QuickWindow.Window {
    id: tip
    title: "Bloody A90 horizontal wheel"
    width: 28
    height: 24
    visible: false
    color: "transparent"
    flags: Qt.ToolTip | Qt.FramelessWindowHint | Qt.WindowStaysOnTopHint | Qt.WindowDoesNotAcceptFocus | Qt.WindowTransparentForInput
    property bool pending: false
    property int missed: 0

    function stop() {
        tip.visible = false;
        timer.stop();
        DBus.SessionBus.asyncCall({ service: "org.kde.KWin", path: "/Scripting", iface: "org.kde.kwin.Scripting", member: "unloadScript", arguments: ["A90_PLUGIN"] });
    }

    Timer {
        id: timer
        interval: 100
        running: true
        repeat: true
        triggeredOnStart: true
        onTriggered: {
            if (tip.pending) {
                if (++tip.missed >= 10) tip.stop();
                return;
            }
            tip.pending = true;
            DBus.SessionBus.asyncCall({ service: "A90_DESTINATION", path: "/org/bloody/A90/Wheel", iface: "org.bloody.A90.Wheel", member: "DirectionFrame", arguments: [] }, function(reply) {
                tip.pending = false;
                tip.missed = 0;
                const values = String(reply.value).split(',');
                if (values.length !== 2) { tip.visible = false; return; }
                const x = Number(values[0]);
                const y = Number(values[1]);
                const output = Workspace.screenAt(Qt.point(x, y));
                if (!output || !isFinite(x) || !isFinite(y)) { tip.visible = false; return; }
                const r = output.geometry;
                tip.x = Math.max(r.x, Math.min(x + 16, r.x + r.width - tip.width));
                tip.y = Math.max(r.y, Math.min(y + 16, r.y + r.height - tip.height));
                tip.visible = true;
            }, function(reply) { tip.stop(); });
        }
    }
    Rectangle {
        anchors.fill: parent
        radius: 5
        color: "#dc24252a"
        border.color: "#ed444d"
        Text { anchors.centerIn: parent; text: "↔"; color: "white"; font.pixelSize: 20 }
    }
}
