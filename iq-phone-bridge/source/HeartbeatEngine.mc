// Builds the bridge heartbeat in small slices.
//
// The FR235 watchdog allows roughly two of the original SHA-256 compressions per
// callback, and tripping it during startup reboots the watch. step() therefore
// runs exactly one bounded unit of work (one SHA-256 compression, one AES block,
// or the key schedule) and the owner calls it once per timer tick.
//
// Steps 1-4 derive the per-key state once; steps 5-10 build one heartbeat.

import Toybox.Lang;

class HeartbeatEngine {
    private var _key as Array<Number>;
    private var _roundKeys as Array<Number> = [] as Array<Number>;
    private var _macKey as Array<Number> = [] as Array<Number>;
    private var _innerState as Array<Number> = [] as Array<Number>;
    private var _outerState as Array<Number> = [] as Array<Number>;
    private var _plain as Array<Number> = [] as Array<Number>;
    private var _padded as Array<Number> = [] as Array<Number>;
    private var _digest as Array<Number> = [] as Array<Number>;
    private var _iv as Array<Number> = [] as Array<Number>;
    private var _block0 as Array<Number> = [] as Array<Number>;
    private var _authenticated as Array<Number> = [] as Array<Number>;
    private var _reply as Array<Number> = [] as Array<Number>;
    private var _step as Number = 1;
    private var _error as String? = null;

    function initialize(key as Array<Number>) {
        _key = key;
    }

    function isReady() as Boolean {
        return _roundKeys.size() == 240 && _innerState.size() == 8 && _outerState.size() == 8;
    }

    function error() as String? {
        return _error;
    }

    // The 81-byte heartbeat once steps 5-10 have completed; empty otherwise.
    function reply() as Array<Number> {
        return _reply;
    }

    // Starts a heartbeat, replacing any heartbeat in progress: the PC only accepts
    // an echo of its latest challenge.
    function begin(counter as Number, tsSec as Number, challenge as Array<Number>) as Boolean {
        if (!isReady() || challenge.size() != 16) { return false; }
        var plain = new [24] as Array<Number>;
        for (var i = 0; i < 4; i++) {
            plain[i] = (counter >> (i * 8)) & 0xff;
            plain[4 + i] = (tsSec >> (i * 8)) & 0xff;
        }
        for (var j = 0; j < 16; j++) { plain[8 + j] = challenge[j]; }
        // PKCS#7: 24 bytes of plaintext leave 8 bytes of padding, each 0x08.
        var padded = new [32] as Array<Number>;
        for (var k = 0; k < 32; k++) { padded[k] = (k < 24) ? plain[k] : 8; }
        _plain = plain;
        _padded = padded;
        _reply = [] as Array<Number>;
        _error = null;
        _step = 5;
        return true;
    }

    // Runs one slice. Returns true while more slices remain; false when idle,
    // finished, or failed (check error()).
    function step() as Boolean {
        var current = _step;
        if (current == 1) {
            _roundKeys = BridgeCrypto.aesRoundKeys(_key, BridgeCrypto.aesSbox());
            return advance(_roundKeys.size() == 240, "AES key schedule");
        } else if (current == 2) {
            // "crabnnb-mac"
            var label = [99, 114, 97, 98, 110, 110, 98, 45, 109, 97, 99] as Array<Number>;
            _macKey = BridgeCrypto.sha256Finish(BridgeCrypto.sha256InitialState(), BridgeCrypto.concat(label, _key), 0);
            return advance(_macKey.size() == 32, "MAC key");
        } else if (current == 3) {
            _innerState = BridgeCrypto.hmacPadState(_macKey, 0x36);
            return advance(_innerState.size() == 8, "HMAC inner state");
        } else if (current == 4) {
            _outerState = BridgeCrypto.hmacPadState(_macKey, 0x5c);
            _key = [] as Array<Number>;
            _macKey = [] as Array<Number>;
            return finish(_outerState.size() == 8, "HMAC outer state");
        } else if (current == 5) {
            _digest = BridgeCrypto.sha256Finish(_innerState, _plain, 64);
            return advance(_digest.size() == 32, "IV inner hash");
        } else if (current == 6) {
            _iv = BridgeCrypto.slice(BridgeCrypto.sha256Finish(_outerState, _digest, 64), 0, 16);
            return advance(_iv.size() == 16, "IV outer hash");
        } else if (current == 7) {
            _block0 = BridgeCrypto.aesCbcBlock(_padded, 0, _iv, _roundKeys, BridgeCrypto.aesSbox());
            return advance(_block0.size() == 16, "AES block 1");
        } else if (current == 8) {
            var block1 = BridgeCrypto.aesCbcBlock(_padded, 16, _block0, _roundKeys, BridgeCrypto.aesSbox());
            if (block1.size() == 16) {
                var mode = [1] as Array<Number>;
                _authenticated = BridgeCrypto.concat(BridgeCrypto.concat(mode, _iv), BridgeCrypto.concat(_block0, block1));
            }
            return advance(block1.size() == 16, "AES block 2");
        } else if (current == 9) {
            _digest = BridgeCrypto.sha256Finish(_innerState, _authenticated, 64);
            return advance(_digest.size() == 32, "tag inner hash");
        } else if (current == 10) {
            var tag = BridgeCrypto.sha256Finish(_outerState, _digest, 64);
            if (tag.size() == 32) { _reply = BridgeCrypto.concat(_authenticated, tag); }
            return finish(tag.size() == 32, "tag outer hash");
        }
        return false;
    }

    private function advance(ok as Boolean, what as String) as Boolean {
        if (!ok) { return finish(false, what); }
        _step += 1;
        return true;
    }

    private function finish(ok as Boolean, what as String) as Boolean {
        if (!ok) { _error = what + " failed"; }
        _step = 0;
        return false;
    }
}
