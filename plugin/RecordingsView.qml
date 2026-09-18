import QtQuick
import Quickshell
import qs.Commons
import qs.Ui
import "Format.js" as Format

Item {
  id: root
  property var app
  // A delete needs a second click on the same row.
  property int confirmDelete: -1

  readonly property var recordings: app.recordings
  readonly property real totalBytes: recordings.reduce(function(sum, r) { return sum + r.bytes }, 0)

  function folderOf(path) { return path.substring(0, path.lastIndexOf("/")) }

  Column {
    anchors.fill: parent
    anchors.topMargin: Style.space(4)
    spacing: Style.space(10)

    Item {
      width: parent.width
      height: Math.max(toggle.height, autoLabels.implicitHeight)
      Column {
        id: autoLabels
        anchors.left: parent.left
        anchors.right: toggle.left
        anchors.verticalCenter: parent.verticalCenter
        Text {
          text: "Auto-record calls"
          color: app.fg; font.family: app.font; font.pixelSize: Style.font.body; font.bold: true
        }
        Text {
          width: parent.width
          wrapMode: Text.WordWrap
          text: "Records every call while its audio is on the laptop. Check the recording laws where you are."
          color: app.fg; opacity: 0.55; font.family: app.font; font.pixelSize: Style.font.caption
        }
      }
      ToggleSwitch {
        id: toggle
        anchors.right: parent.right
        anchors.verticalCenter: parent.verticalCenter
        checked: app.autoRecord
        foreground: app.fg
        onToggled: app.daemon.act("set_auto_record", { enabled: !app.autoRecord })
      }
    }

    PanelSeparator { foreground: app.fg }

    Text {
      visible: list.count === 0
      width: parent.width
      topPadding: Style.space(40)
      horizontalAlignment: Text.AlignHCenter
      text: "No recordings yet"
      color: app.fg; opacity: 0.5; font.family: app.font; font.pixelSize: Style.font.bodySmall
    }

    ListView {
      id: list
      width: parent.width
      height: parent.height - toggle.height - footer.height - Style.space(50)
      clip: true
      model: root.recordings
      spacing: Style.space(2)
      boundsBehavior: Flickable.StopAtBounds

      delegate: ListRow {
        id: row
        required property var modelData
        width: list.width
        height: Style.space(44)
        app: root.app
        onClicked: Quickshell.execDetached(["xdg-open", row.modelData.path])

        Text {
          id: playIcon
          anchors.left: parent.left
          anchors.verticalCenter: parent.verticalCenter
          width: Style.space(24)
          text: app.icons.play
          color: app.fg
          font.family: app.font; font.pixelSize: Style.font.heading
        }
        Column {
          anchors.left: playIcon.right
          anchors.leftMargin: Style.space(8)
          anchors.right: actions.left
          anchors.verticalCenter: parent.verticalCenter
          Text {
            width: parent.width
            elide: Text.ElideRight
            text: row.modelData.name || row.modelData.number
            color: app.fg; font.family: app.font; font.pixelSize: Style.font.body; font.bold: true
          }
          Text {
            width: parent.width
            elide: Text.ElideRight
            text: Format.dayLabel(row.modelData.started_at, app.now) + " " + Format.timeOfDay(row.modelData.started_at)
                  + " · " + Format.clock(row.modelData.duration) + " · " + Format.size(row.modelData.bytes)
            color: app.fg; opacity: 0.55; font.family: app.font; font.pixelSize: Style.font.caption
          }
        }
        Row {
          id: actions
          anchors.right: parent.right
          anchors.verticalCenter: parent.verticalCenter
          spacing: Style.space(12)
          Text {
            text: app.icons.folder
            color: app.fg
            opacity: folderMouse.containsMouse ? 1 : 0.6
            font.family: app.font; font.pixelSize: Style.font.title
            MouseArea {
              id: folderMouse
              anchors.fill: parent
              hoverEnabled: true
              cursorShape: Qt.PointingHandCursor
              onClicked: Quickshell.execDetached(["xdg-open", root.folderOf(row.modelData.path)])
            }
          }
          Text {
            readonly property bool confirming: root.confirmDelete === row.modelData.id
            text: confirming ? "Delete?" : app.icons.trash
            color: app.urgent
            opacity: confirming || trashMouse.containsMouse ? 1 : 0.6
            font.family: app.font
            font.pixelSize: confirming ? Style.font.caption : Style.font.title
            font.bold: confirming
            MouseArea {
              id: trashMouse
              anchors.fill: parent
              hoverEnabled: true
              cursorShape: Qt.PointingHandCursor
              onClicked: {
                if (parent.confirming) {
                  root.confirmDelete = -1
                  app.daemon.send("delete_recording", { id: row.modelData.id }, function(ok, error) {
                    if (!ok) app.showError(error)
                    app.daemon.send("get_recordings")
                  })
                } else {
                  root.confirmDelete = row.modelData.id
                }
              }
            }
          }
        }
      }
    }

    Text {
      id: footer
      visible: root.recordings.length > 0
      text: root.recordings.length + " recordings · " + Format.size(root.totalBytes) + " · " + root.folderOf(root.recordings[0] ? root.recordings[0].path : "").replace(Quickshell.env("HOME"), "~")
      color: app.fg; opacity: 0.45; font.family: app.font; font.pixelSize: Style.font.caption
    }
  }
}
