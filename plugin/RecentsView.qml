import QtQuick
import qs.Commons
import qs.Ui
import "Format.js" as Format

Item {
  id: root
  property var app
  property string filter: "all"
  readonly property var model: Format.recentsWithHeaders(
    app.recents.filter(function(r) { return root.filter !== "missed" || r.kind === "missed" }), app.now)

  function kindIcon(k) { return k === "incoming" ? app.icons.incoming : k === "outgoing" ? app.icons.outgoing : app.icons.missed }

  Column {
    anchors.fill: parent
    anchors.topMargin: Style.space(4)
    spacing: Style.space(8)

    ButtonGroup {
      id: filterGroup
      spacing: Style.space(4)
      foreground: app.fg
      fontFamily: app.font
      fontSize: Style.font.caption
      focusable: false
      value: root.filter
      options: [{ value: "all", label: "All" }, { value: "missed", label: "Missed" }]
      onChanged: function(v) { root.filter = v }
    }

    Text {
      visible: list.count === 0
      width: parent.width
      topPadding: Style.space(40)
      horizontalAlignment: Text.AlignHCenter
      text: root.filter === "missed" ? "No missed calls" : "No calls yet"
      color: app.fg; opacity: 0.5; font.family: app.font; font.pixelSize: Style.font.bodySmall
    }

    ListView {
      id: list
      width: parent.width
      height: parent.height - filterGroup.height - Style.space(8)
      clip: true
      model: root.model
      boundsBehavior: Flickable.StopAtBounds

      delegate: Column {
        id: row
        required property var modelData
        width: list.width
        readonly property bool missed: modelData.kind === "missed"

        Text {
          visible: row.modelData.first
          topPadding: Style.space(8)
          bottomPadding: Style.space(2)
          leftPadding: Style.space(8)
          text: row.modelData.dayLabel.toUpperCase()
          color: app.fg; opacity: 0.5
          font.family: app.font; font.pixelSize: Style.font.caption; font.bold: true; font.letterSpacing: 1.2
        }

        ListRow {
          width: parent.width
          height: Style.space(44)
          app: root.app
          enabled: row.modelData.number !== ""
          onClicked: app.dial(row.modelData.number)

          Text {
            id: kind
            anchors.left: parent.left
            anchors.verticalCenter: parent.verticalCenter
            width: Style.space(22)
            text: root.kindIcon(row.modelData.kind)
            color: row.missed ? app.urgent : app.fg
            opacity: row.missed ? 1 : 0.6
            font.family: app.font
            font.pixelSize: Style.font.heading
          }
          Column {
            anchors.left: kind.right
            anchors.leftMargin: Style.space(8)
            anchors.right: meta.left
            anchors.rightMargin: Style.space(8)
            anchors.verticalCenter: parent.verticalCenter
            Text {
              width: parent.width
              elide: Text.ElideRight
              text: (row.modelData.name || row.modelData.number || "Private number")
                    + (row.modelData.count > 1 ? "  (" + row.modelData.count + ")" : "")
              color: row.missed ? app.urgent : app.fg
              font.family: app.font; font.pixelSize: Style.font.body; font.bold: true
            }
            Text {
              width: parent.width
              elide: Text.ElideRight
              text: (row.modelData.name ? row.modelData.number : "Unknown") + " · " + row.modelData.kind
              color: app.fg; opacity: 0.55
              font.family: app.font; font.pixelSize: Style.font.caption
            }
          }
          Row {
            id: meta
            anchors.right: parent.right
            anchors.verticalCenter: parent.verticalCenter
            spacing: Style.space(10)
            Text {
              anchors.verticalCenter: parent.verticalCenter
              visible: row.modelData.recording !== null && row.modelData.recording !== undefined
              text: app.icons.record
              color: app.urgent
              font.family: app.font; font.pixelSize: Style.font.bodySmall
              MouseArea {
                anchors.fill: parent
                anchors.margins: -Style.space(6)
                cursorShape: Qt.PointingHandCursor
                onClicked: app.tab = 3
              }
            }
            Column {
              anchors.verticalCenter: parent.verticalCenter
              Text {
                anchors.right: parent.right
                text: Format.timeOfDay(row.modelData.at)
                color: app.fg; opacity: 0.8
                font.family: app.font; font.pixelSize: Style.font.bodySmall
              }
              Text {
                anchors.right: parent.right
                text: row.modelData.duration !== null && row.modelData.duration !== undefined
                      ? Format.clock(row.modelData.duration) : "—"
                color: app.fg; opacity: 0.45
                font.family: app.font; font.pixelSize: Style.font.caption
              }
            }
          }
        }
      }
    }
  }
}
