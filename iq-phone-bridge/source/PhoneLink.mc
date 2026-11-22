// Handles API 1.4 phone-app messages. Both directions use hexadecimal strings
// because the FR235 does not expose Lang.ByteArray (API 3.0+).
//
// The heartbeat is computed by HeartbeatEngine one slice per timer tick, so no
// callback runs long enough to trip the watchdog.

import Toybox.Application;
import Toybox.Communications;
import Toybox.Lang;
import Toybox.System;
import Toybox.Time;
import Toybox.Timer;
import Toybox.WatchUi;

class PhoneLink {
    // Shortest period Connect IQ timers support.
    private const TICK_MS = 50;
    // Counter values reserved per flash write; see send().
    private const COUNTER_RESERVE = 64;


    private var _engine as HeartbeatEngine;
    private var _timer as Timer.Timer;
    private var _counter as Number;
    private var _reserved as Number;
    private var _registered as Boolean = false;
    private var _startedAt as Number = 0;
    private var _sentTimestamp as Number = 0;
    private var _lastError as String? = null;
    private var _lastStage as String = "preparing key";

    function initialize(key as Array<Number>) {
        _engine = new HeartbeatEngine(key);
        var stored = Application.getApp().getProperty("counter");
        _counter = (stored instanceof Number) ? stored : 1;
        _reserved = _counter;
        _timer = new Timer.Timer();
        _timer.start(method(:onTick), TICK_MS, true);
    }

    function lastError() as String? {
        return _lastError;
    }

    function lastStage() as String? {
        return _lastStage;
    }

    function recordStage(stage as String) as Void {
        _lastStage = stage;
        WatchUi.requestUpdate();
    }

    function recordError(error as String) as Void {
        _lastError = error;
        recordStage("stopped");
    }

    function onTick() as Void {
        var more = false;
        try {
            more = _engine.step();
        } catch (ex) {
            _timer.stop();
            recordError("engine exception");
            return;
        }
        if (more) { return; }
        _timer.stop();
        var error = _engine.error();
        if (error != null) { recordError(error); return; }
        if (!_registered) {
            _registered = true;
            Communications.registerForPhoneAppMessages(method(:onMessageFromPhoneApp));
            recordStage("ready");
            return;
        }
        var reply = _engine.reply();
        if (reply.size() == 81) { send(reply); }
    }

    function onMessageFromPhoneApp(message as Communications.PhoneAppMessage) as Void {
        var data = message.data;
        if (!(data instanceof String)) { recordError("bad data type"); return; }
        var challenge = BridgeCrypto.fromHex(data, 16);
        if (challenge.size() != 16) { recordError("bad challenge"); return; }
        _startedAt = System.getTimer();
        _sentTimestamp = unixSeconds();
        if (!_engine.begin(_counter, _sentTimestamp, challenge)) { recordError("key not ready"); return; }
        _lastError = null;
        recordStage("computing");
        _timer.stop();
        _timer.start(method(:onTick), TICK_MS, true);
    }

    // Connect IQ returns Unix seconds here; keep it within the signed 32-bit Number range.
    private function unixSeconds() as Number {
        return Time.now().value();
    }

    private function send(reply as Array<Number>) as Void {
        // The PC rejects non-increasing counters. Persisting an upper bound in
        // blocks keeps that true across restarts and crashes with one flash write
        // per COUNTER_RESERVE heartbeats instead of one per heartbeat.
        if (_counter >= _reserved) {
            _reserved = _counter + COUNTER_RESERVE;
            var app = Application.getApp();
            app.setProperty("counter", _reserved);
            app.saveProperties();
        }
        _counter += 1;
        recordStage("ts=" + _sentTimestamp + " (" + (System.getTimer() - _startedAt) + "ms)");
        Communications.transmit(BridgeCrypto.toHex(reply), null, new BridgeTransmitListener(self));
    }
}

class BridgeTransmitListener extends Communications.ConnectionListener {
    private var _link as PhoneLink;

    function initialize(link as PhoneLink) {
        ConnectionListener.initialize();
        _link = link;
    }

    function onComplete() as Void {
    }

    function onError() as Void {
        _link.recordError("transmit failed");
    }
}
