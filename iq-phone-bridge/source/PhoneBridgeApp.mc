import Toybox.Application;
import Toybox.Lang;
import Toybox.WatchUi;

class PhoneBridgeApp extends Application.AppBase {

    private var _link as PhoneLink;

    function initialize() {
        AppBase.initialize();
    }

    function onStart(state as Dictionary?) as Void {
        var keyHex = Application.Properties.getValue("pairingKey") as String;
        var key = (keyHex != null) ? BridgeCrypto.hexToBytes(keyHex) : null;
        if (key != null) {
            _link = new PhoneLink(key);
        }
    }

    function onStop(state as Dictionary?) as Void {
    }

    function getInitialView() as Array {
        return [new BridgeView(), new BridgeDelegate()];
    }

    function hasKey() as Boolean {
        return _link != null;
    }
}
