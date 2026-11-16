// PhoneLink: receives 16-byte challenges from the phone companion (relayed
// from the PC) and answers with an encrypted heartbeat. Verify the exact
// Communications delegate method names against the Connect IQ SDK.

import Toybox.Communications;
import Toybox.Lang;
import Toybox.Storage;
import Toybox.System;

class PhoneLink extends Communications.CommunicationsDelegate {

    private var _key as ByteArray;
    private var _counter as Number;

    function initialize(key as ByteArray) {
        _key = key;
        var stored = Storage.getValue("counter");
        _counter = (stored == null) ? 1 : stored as Number;
        Communications.registerForPhoneAppMessages(self);
    }

    function onMessageFromPhoneApp(message as ByteArray) as Void {
        if (message == null || message.size() != 16) {
            return;
        }
        var clock = System.getClockTime();
        var tsSec = clock.unixTimestamp as Number; // wall-clock unix seconds; verify field name against SDK
        var hb = BridgeCrypto.buildHeartbeat(_key, _counter, tsSec, message);
        Communications.sendMessage(hb, null, self);
        _counter += 1;
        Storage.setValue("counter", _counter);
    }

    function onMessageToPhoneApp(message as ByteArray) as Void {
        // delivery ack from the phone side; nothing to do
    }
}
