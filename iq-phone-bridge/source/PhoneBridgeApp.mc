import Toybox.Application;
import Toybox.Lang;
import Toybox.WatchUi;

class PhoneBridgeApp extends Application.AppBase {
    private var _link as PhoneLink? = null;

    function initialize() {
        AppBase.initialize();
    }

    function onStart(state as Dictionary?) as Void {
        var embeddedKey = WatchUi.loadResource(Rez.Strings.PairingKeyEmbedded) as String;
        var key = BridgeCrypto.fromHex(embeddedKey, 32);
        var storedKey = Application.getApp().getProperty("pairingKey");
        if (storedKey instanceof String) {
            var configuredKey = BridgeCrypto.fromHex(storedKey, 32);
            if (configuredKey != null) { key = configuredKey; }
        }
        if (key != null) {
            var prepared = BridgeCrypto.prepareKey(key);
            if (prepared != null) { _link = new PhoneLink(prepared); }
        }
    }

    function onStop(state as Dictionary?) as Void {
    }

    function getInitialView() as [WatchUi.Views] or [WatchUi.Views, WatchUi.InputDelegates] {
        return [new BridgeView(), new BridgeDelegate()];
    }

    function hasKey() as Boolean {
        return _link != null;
    }

    function lastError() as String? {
        return (_link == null) ? null : _link.lastError();
    }

    function lastStage() as String? {
        return (_link == null) ? null : _link.lastStage();
    }
}
