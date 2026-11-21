// Handles API 1.4 phone-app messages. Both directions use hexadecimal strings
// because the FR235 does not expose Lang.ByteArray (API 3.0+).

import Toybox.Application;
import Toybox.Lang;
import Toybox.Communications;
import Toybox.Time;

class PhoneLink {
    private var _key as Array<Number>;
    private var _counter as Number;

    function initialize(key as Array<Number>) {
        _key = key;
        var app = Application.getApp();
        var stored = app.getProperty("counter");
        _counter = (stored == null) ? 1 : stored as Number;
        Communications.registerForPhoneAppMessages(method(:onMessageFromPhoneApp));
    }

    function onMessageFromPhoneApp(message as Communications.PhoneAppMessage) as Void {
        if (message.data == null) { return; }
        var challengeHex = message.data as String;
        if (challengeHex.length() != 32) { return; }
        var challenge = BridgeCrypto.fromHex(challengeHex, 16);
        if (challenge == null) { return; }

        var timestamp = Time.now().value();
        var heartbeat = BridgeCrypto.buildHeartbeat(_key, _counter, timestamp, challenge);
        if (heartbeat.size() != 81) { return; }

        _counter += 1;
        Application.getApp().setProperty("counter", _counter);
        Communications.transmit(BridgeCrypto.toHex(heartbeat), null, new BridgeTransmitListener());
    }
}

class BridgeTransmitListener extends Communications.ConnectionListener {
    function initialize() {
        ConnectionListener.initialize();
    }

    function onComplete() as Void {
    }

    function onError() as Void {
    }
}
