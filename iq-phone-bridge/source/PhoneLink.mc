// Handles API 1.4 phone-app messages. Both directions use hexadecimal strings
// because the FR235 does not expose Lang.ByteArray (API 3.0+).

import Toybox.Application;
import Toybox.Lang;
import Toybox.Communications;
import Toybox.Time;

class PhoneLink {
    private var _prepared as Array<Array<Number>>;
    private var _counter as Number;
    private var _lastError as String? = null;
    private var _lastStage as String? = null;

    function initialize(prepared as Array<Array<Number>>) {
        _prepared = prepared;
        var app = Application.getApp();
        var stored = app.getProperty("counter");
        _counter = (stored instanceof Number) ? stored : 1;
        var stage = app.getProperty("debugStage");
        _lastStage = (stage instanceof String) ? stage : "ready";
        var error = app.getProperty("debugError");
        _lastError = (error instanceof String && error.length() > 0) ? error : null;
        Communications.registerForPhoneAppMessages(method(:onMessageFromPhoneApp));
    }

    function onMessageFromPhoneApp(message as Communications.PhoneAppMessage) as Void {
        _lastError = null;
        try {
            Application.getApp().setProperty("debugError", "");
        } catch (ex) {
        }
        recordStage("challenge received");
        try {
            answerChallenge(message.data);
        } catch (ex instanceof Lang.Exception) {
            recordError(ex.getErrorMessage());
        } catch (ex) {
            recordError("unknown error");
        }
    }

    function lastError() as String? {
        return _lastError;
    }

    function lastStage() as String? {
        return _lastStage;
    }

    function recordStage(stage as String) as Void {
        _lastStage = stage;
        BridgeCrypto.checkpoint(stage);
    }

    function recordError(error as String) as Void {
        _lastError = error;
        try {
            Application.getApp().setProperty("debugError", error);
        } catch (ex) {
        }
        recordStage("handler error");
    }

    private function answerChallenge(data as Object?) as Void {
        if (!(data instanceof String)) { recordError("bad data type"); return; }
        if (data.length() != 32) { recordError("bad challenge length"); return; }
        var challenge = BridgeCrypto.fromHex(data, 16);
        if (challenge == null) { recordError("bad challenge hex"); return; }

        recordStage("building heartbeat");
        var timestamp = Time.now().value();
        var heartbeat = BridgeCrypto.buildHeartbeat(_prepared, _counter, timestamp, challenge);
        if (heartbeat.size() != 81) { recordError("heartbeat size"); return; }

        _counter += 1;
        recordStage("saving counter");
        Application.getApp().setProperty("counter", _counter);
        recordStage("transmitting");
        Communications.transmit(BridgeCrypto.toHex(heartbeat), null, new BridgeTransmitListener(self));
        _lastError = null;
    }
}

class BridgeTransmitListener extends Communications.ConnectionListener {
    private var _link as PhoneLink;

    function initialize(link as PhoneLink) {
        ConnectionListener.initialize();
        _link = link;
    }

    function onComplete() as Void {
        _link.recordStage("transmit complete");
    }

    function onError() as Void {
        _link.recordError("transmit failed");
    }
}
