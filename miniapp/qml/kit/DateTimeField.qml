// A date-and-time field: click it for a small panel under the field with a month grid,
// hours and minutes, and Done / Clear. Uses the shared look and the app's date helpers.
// Plain QtQuick only. Opening it makes the field taller (the panel sits inside it), so the
// form below moves down instead of being drawn over.
import QtQuick
import QtQuick.Layouts
import "../logic.mjs" as Logic

Item {
    id: field
    property Look look: Look {}
    property string hint: ""
    property int ms: -1                 // epoch ms, or -1 for empty (two-way)
    property bool open: false
    signal changed()

    implicitHeight: box.height + (open ? panel.height + 4 : 0)
    implicitWidth: 0

    Rectangle {
      id: box
      anchors { left: parent.left; right: parent.right; top: parent.top }
      height: 38
      radius: 6
      color: look.slate
      border.width: 1
      border.color: field.open || area.containsMouse ? look.brass : look.line

      Text {
        anchors { left: parent.left; right: parent.right; verticalCenter: parent.verticalCenter; leftMargin: 12; rightMargin: 12 }
        textFormat: Text.PlainText
        text: field.ms >= 0 ? Logic.dateTimeText(field.ms) : field.hint
        color: field.ms >= 0 ? look.ink : look.faint
        font.family: look.sans
        font.pixelSize: look.px(16)
        elide: Text.ElideRight
    }
    MouseArea {
        id: area
        anchors.fill: parent
        hoverEnabled: true
        cursorShape: Qt.PointingHandCursor
        onClicked: field.open ? field.close() : field.show()
      }
    }

    function pad2(n) { return (n < 10 ? "0" : "") + n }
    function monthName(m) {
        var names = ["January", "February", "March", "April", "May", "June",
            "July", "August", "September", "October", "November", "December"]
        return names[m]
    }
    function show() {
        if (field.ms >= 0) {
            var p = Logic.partsOf(field.ms)
            panel.year = p.year; panel.month = p.month; panel.day = p.day
            panel.hour = p.hour; panel.minute = p.minute
        } else {
            var now = new Date()
            panel.year = now.getFullYear(); panel.month = now.getMonth(); panel.day = now.getDate()
            panel.hour = 9; panel.minute = 0
        }
        hourInput.text = pad2(panel.hour)
        minuteInput.text = pad2(panel.minute)
        field.open = true
        panel.forceActiveFocus()
    }
    function close() { field.open = false }
    function shiftMonth(by) {
        var d = new Date(panel.year, panel.month + by, 1)
        panel.year = d.getFullYear()
        panel.month = d.getMonth()
        var last = new Date(panel.year, panel.month + 1, 0).getDate()
        if (panel.day > last) panel.day = last
    }
    function bumpHour(by) {
        panel.hour = (panel.hour + by + 24) % 24
        hourInput.text = pad2(panel.hour)
    }
    function bumpMinute(by) {
        panel.minute = (panel.minute + by + 60) % 60
        minuteInput.text = pad2(panel.minute)
    }
    function setHour(text) {
        var n = parseInt(text, 10)
        if (!isFinite(n)) n = panel.hour
        panel.hour = Math.max(0, Math.min(23, n))
        hourInput.text = pad2(panel.hour)
    }
    function setMinute(text) {
        var n = parseInt(text, 10)
        if (!isFinite(n)) n = panel.minute
        n = Math.round(n / 5) * 5
        panel.minute = Math.max(0, Math.min(55, n))
        minuteInput.text = pad2(panel.minute)
    }
    function done() {
        field.ms = Logic.atLocal(panel.year, panel.month, panel.day, panel.hour, panel.minute)
        field.changed()
        close()
    }
    function clear() {
        field.ms = -1
        field.changed()
        close()
    }
    Keys.onEscapePressed: if (field.open) field.close()

    Rectangle {
        id: panel
        visible: field.open
        z: 10
        anchors { top: box.bottom; topMargin: 4 }
        x: 0
        width: Math.max(field.width, look.px(300))
        height: inner.implicitHeight + 24
        radius: 8
        color: look.card
        border.width: 1
        border.color: look.line
        focus: true
        Keys.onEscapePressed: field.close()

        property int year: 2026
        property int month: 0
        property int day: 1
        property int hour: 9
        property int minute: 0
        readonly property int picked: Logic.atLocal(year, month, day, 0, 0)
        readonly property int today: {
            var d = new Date()
            return Logic.atLocal(d.getFullYear(), d.getMonth(), d.getDate(), 0, 0)
        }

        ColumnLayout {
            id: inner
            anchors { left: parent.left; right: parent.right; top: parent.top; margins: 12 }
            spacing: 8

            RowLayout {
                Layout.fillWidth: true
                spacing: 8
                Link { look: field.look; text: "‹"; font.pixelSize: look.px(20); onPressed: field.shiftMonth(-1) }
                Text {
                    Layout.fillWidth: true
                    horizontalAlignment: Text.AlignHCenter
                    textFormat: Text.PlainText
                    text: field.monthName(panel.month) + " " + panel.year
                    color: look.ink
                    font.family: look.sans
                    font.pixelSize: look.px(16)
                    font.weight: Font.DemiBold
                }
                Link { look: field.look; text: "›"; font.pixelSize: look.px(20); onPressed: field.shiftMonth(1) }
            }

            RowLayout {
                Layout.fillWidth: true
                spacing: 2
                Repeater {
                    model: ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"]
                    Text {
                        Layout.fillWidth: true
                        Layout.preferredWidth: 1
                        horizontalAlignment: Text.AlignHCenter
                        textFormat: Text.PlainText
                        text: modelData
                        color: look.dim
                        font.family: look.sans
                        font.pixelSize: look.px(12)
                    }
                }
            }

            GridLayout {
                Layout.fillWidth: true
                columns: 7
                rowSpacing: 2
                columnSpacing: 2
                Repeater {
                    model: Logic.monthGrid(panel.year, panel.month)
                    delegate: Rectangle {
                        required property var modelData
                        readonly property bool picked: modelData.date === panel.picked
                        Layout.fillWidth: true
                        Layout.preferredWidth: 1
                        Layout.preferredHeight: look.px(26)
                        radius: 4
                        color: picked ? look.brass : (cellArea.containsMouse ? look.raised : "transparent")
                        border.width: !picked && modelData.date === panel.today ? 1 : 0
                        border.color: look.brass
                        Text {
                            anchors.centerIn: parent
                            textFormat: Text.PlainText
                            text: modelData.day
                            color: picked ? look.slate : (modelData.inMonth ? look.ink : look.faint)
                            font.family: look.sans
                            font.pixelSize: look.px(13)
                        }
                        MouseArea {
                            id: cellArea
                            anchors.fill: parent
                            hoverEnabled: true
                            cursorShape: Qt.PointingHandCursor
                            onClicked: {
                                var d = Logic.partsOf(modelData.date)
                                panel.year = d.year; panel.month = d.month; panel.day = d.day
                            }
                        }
                    }
                }
            }

            RowLayout {
                Layout.fillWidth: true
                spacing: 6
                Text { textFormat: Text.PlainText; text: "Hours"; color: look.dim; font.family: look.sans; font.pixelSize: look.px(14) }
                Link { look: field.look; text: "−"; onPressed: field.bumpHour(-1) }
                Field { id: hourInput; look: field.look; Layout.preferredWidth: look.px(44); onAccepted: field.setHour(text) }
                Link { look: field.look; text: "+"; onPressed: field.bumpHour(1) }
            }
            RowLayout {
                Layout.fillWidth: true
                spacing: 6
                Text { textFormat: Text.PlainText; text: "Minutes"; color: look.dim; font.family: look.sans; font.pixelSize: look.px(14) }
                Link { look: field.look; text: "−"; onPressed: field.bumpMinute(-5) }
                Field { id: minuteInput; look: field.look; Layout.preferredWidth: look.px(44); onAccepted: field.setMinute(text) }
                Link { look: field.look; text: "+"; onPressed: field.bumpMinute(5) }
            }

            RowLayout {
                Layout.fillWidth: true
                spacing: 12
                Action { look: field.look; label: "Done"; strong: true; onPressed: field.done() }
                Link { look: field.look; text: "Clear"; onPressed: field.clear() }
            }
        }
    }
}
