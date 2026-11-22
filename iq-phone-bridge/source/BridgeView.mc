import Toybox.Application;
import Toybox.Graphics;
import Toybox.WatchUi;
import Toybox.Lang;

class BridgeView extends WatchUi.View {

    function initialize() {
        View.initialize();
    }

    function onUpdate(dc as Graphics.Dc) as Void {
        dc.setColor(Graphics.COLOR_BLACK, Graphics.COLOR_BLACK);
        dc.clear();
        dc.setColor(Graphics.COLOR_WHITE, Graphics.COLOR_TRANSPARENT);
        var app = Application.getApp() as PhoneBridgeApp;
        var msg = app.hasKey() ? WatchUi.loadResource(Rez.Strings.StatusWaiting)
                               : WatchUi.loadResource(Rez.Strings.StatusNoKey);
        var err = app.lastError();
        if (err != null) { msg = err; }
        dc.drawText(dc.getWidth() / 2, dc.getHeight() / 2 - 12, Graphics.FONT_SMALL, msg,
                    Graphics.TEXT_JUSTIFY_CENTER | Graphics.TEXT_JUSTIFY_VCENTER);
        var stage = app.lastStage();
        if (stage != null) {
            dc.drawText(dc.getWidth() / 2, dc.getHeight() / 2 + 25, Graphics.FONT_XTINY, stage,
                        Graphics.TEXT_JUSTIFY_CENTER | Graphics.TEXT_JUSTIFY_VCENTER);
        }
    }
}

class BridgeDelegate extends WatchUi.BehaviorDelegate {
    function initialize() {
        BehaviorDelegate.initialize();
    }
}
