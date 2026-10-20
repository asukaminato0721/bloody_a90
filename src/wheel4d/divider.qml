import QtQuick
import QtQuick.Window as QuickWindow
import org.kde.kwin
import org.kde.plasma.workspace.dbus 1.0 as DBus

QuickWindow.Window {
    id: tip
    title: "A90_PLUGIN"
    width: 32
    height: 24
    visible: false
    color: "transparent"
    flags: Qt.Tool | Qt.FramelessWindowHint | Qt.X11BypassWindowManagerHint | Qt.WindowStaysOnTopHint | Qt.WindowDoesNotAcceptFocus
    property bool pending: false
    property bool dragging: false
    property int missed: 0
    property string app: ""
    property real leftEdge: 0
    property real regionWidth: 1
    property int ratio: 900
    property string dragApp: ""
    property real pressX: 0
    property int pressRatio: 900

    function stop() {
        tip.visible = false;
        timer.stop();
        DBus.SessionBus.asyncCall({ service: "org.kde.KWin", path: "/Scripting", iface: "org.kde.kwin.Scripting", member: "unloadScript", arguments: ["A90_PLUGIN"] });
    }
    function moveDivider() {
        const delta = Workspace.cursorPos.x - tip.pressX;
        tip.ratio = Math.round(Math.max(100, Math.min(900, tip.pressRatio + delta * 1000 / tip.regionWidth)));
        tip.x = tip.leftEdge + tip.regionWidth * tip.ratio / 1000 - tip.width / 2;
    }
    Timer {
        id: timer
        interval: 100
        running: true
        repeat: true
        triggeredOnStart: true
        onTriggered: {
            if (tip.pending) { if (++tip.missed >= 10) tip.stop(); return; }
            tip.pending = true;
            DBus.SessionBus.asyncCall({ service: "A90_DESTINATION", path: "/org/bloody/A90/Wheel", iface: "org.bloody.A90.Wheel", member: "DividerFrame", arguments: [] }, function(reply) {
                tip.pending = false;
                tip.missed = 0;
                if (tip.dragging) return;
                const v = reply.values;
                if (!v[0]) { tip.visible = false; return; }
                tip.leftEdge = v[3];
                tip.regionWidth = v[4];
                tip.app = v[5];
                tip.ratio = v[6];
                tip.x = v[1];
                tip.y = v[2];
                tip.visible = true;
            }, function(reply) { tip.stop(); });
        }
    }
    Rectangle {
        anchors.fill: parent
        radius: 4
        color: mouse.pressed ? "#e94545" : "#e82a2b30"
        border.color: "#e94545"
        Text { anchors.centerIn: parent; text: "↔│"; color: "white"; font.pixelSize: 17 }
    }
    MouseArea {
        id: mouse
        anchors.fill: parent
        cursorShape: Qt.SizeHorCursor
        acceptedButtons: Qt.LeftButton
        onPressed: {
            tip.dragging = true;
            tip.dragApp = tip.app;
            tip.pressX = Workspace.cursorPos.x;
            tip.pressRatio = tip.ratio;
        }
        onPositionChanged: { if (pressed) tip.moveDivider(); }
        onCanceled: tip.dragging = false
        onReleased: {
            tip.moveDivider();
            tip.dragging = false;
            DBus.SessionBus.asyncCall({ service: "A90_DESTINATION", path: "/org/bloody/A90/Wheel", iface: "org.bloody.A90.Wheel", member: "UpdateDivider", arguments: [tip.dragApp, tip.ratio] }, function(reply) { if (!reply.value) tip.stop(); }, function(reply) { tip.stop(); });
        }
    }
}
