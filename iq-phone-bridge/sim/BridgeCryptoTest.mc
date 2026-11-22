import Toybox.Communications;
import Toybox.Lang;
import Toybox.System;
import Toybox.Timer;

// Runs one small phase per timer tick, like the production PhoneLink, so the
// checks stay inside the watchdog budget that the FR235 enforces per callback.
class BridgeCryptoTest {
    private const RESULT_URL = "http://127.0.0.1:__CIQ_TEST_PORT__/";
    private static var _keep as BridgeCryptoTest? = null;

    private var _failures as Array<String> = [];
    private var _checks as Number = 0;
    private var _phase as Number = 0;
    private var _timer as Timer.Timer;
    private var _inner as Array<Number> = [] as Array<Number>;
    private var _outer as Array<Number> = [] as Array<Number>;
    private var _roundKeys as Array<Number> = [] as Array<Number>;
    private var _aesData as Array<Number> = [] as Array<Number>;
    private var _aesOut as Array<Number> = [] as Array<Number>;
    private var _aesPrevious as Array<Number> = [] as Array<Number>;
    private var _engine as HeartbeatEngine? = null;
    private var _engineSteps as Number = 0;

    function initialize() {
        _timer = new Timer.Timer();
    }

    function run() as Void {
        _keep = self;
        _timer.start(method(:onTick), 50, true);
    }

    private function check(name as String, passed as Boolean) as Void {
        _checks += 1;
        if (!passed) { _failures.add(name); }
    }

    private function checkEqual(name as String, expected as String, actual as String) as Void {
        check(name, expected.equals(actual));
    }

    private function filled(size as Number, value as Number) as Array<Number> {
        var out = new [size] as Array<Number>;
        for (var i = 0; i < size; i++) { out[i] = value; }
        return out;
    }

    // Multi-block SHA-256 built on the production compression function.
    private function sha256(input as Array<Number>) as Array<Number> {
        var size = input.size();
        var paddedSize = ((size + 9 + 63) / 64) * 64;
        var data = new [paddedSize] as Array<Number>;
        for (var i = 0; i < paddedSize; i++) { data[i] = (i < size) ? input[i] : 0; }
        data[size] = 0x80;
        var bits = size * 8;
        data[paddedSize - 2] = (bits >> 8) & 0xff;
        data[paddedSize - 1] = bits & 0xff;
        var hash = BridgeCrypto.sha256InitialState();
        for (var block = 0; block < paddedSize; block += 64) {
            BridgeCrypto.sha256Compress(hash, data, block);
        }
        return BridgeCrypto.sha256Bytes(hash);
    }

    private function hmacMessage() as Array<Number> {
        // "The quick brown fox jumps over the lazy dog"
        return [84,104,101,32,113,117,105,99,107,32,98,114,111,119,110,32,102,111,120,32,106,117,109,112,115,32,111,118,101,114,32,116,104,101,32,108,97,122,121,32,100,111,103] as Array<Number>;
    }

    private function hmacWithStates() as String {
        var digest = BridgeCrypto.sha256Finish(_inner, hmacMessage(), 64);
        return BridgeCrypto.toHex(BridgeCrypto.sha256Finish(_outer, digest, 64));
    }

    private function aesStep(offset as Number) as Void {
        var block = BridgeCrypto.aesCbcBlock(_aesData, offset, _aesPrevious, _roundKeys, BridgeCrypto.aesSbox());
        _aesPrevious = block;
        _aesOut = BridgeCrypto.concat(_aesOut, block);
    }

    function onTick() as Void {
        try {
            runPhase();
        } catch (ex instanceof Lang.Exception) {
            var message = ex.getErrorMessage();
            finish("CRYPTO_TESTS_FAILED exception in phase " + _phase + ": " + ((message != null) ? message : "unknown"));
        }
    }

    private function runPhase() as Void {
        var phase = _phase;
        _phase = phase + 1;
        if (phase == 0) {
            var big = 0x7fffffff;
            check("number_addition_wraps", (big + 1) < 0);
            checkEqual("hex_parsing", "00a1ff", BridgeCrypto.toHex(BridgeCrypto.fromHex("00A1ff", 3)));
            check("malformed_hex_rejected", BridgeCrypto.fromHex("abc", 2).size() == 0 && BridgeCrypto.fromHex("00xz", 2).size() == 0);
            check("slice_out_of_range_rejected", BridgeCrypto.slice([1, 2, 3] as Array<Number>, 1, 4).size() == 0);
            var empty = [] as Array<Number>;
            checkEqual("sha256_empty", IqVectors.SHA256_EMPTY, BridgeCrypto.toHex(sha256(empty)));
            checkEqual("sha256_finish_empty", IqVectors.SHA256_EMPTY, BridgeCrypto.toHex(BridgeCrypto.sha256Finish(BridgeCrypto.sha256InitialState(), empty, 0)));
        } else if (phase == 1) {
            var abc = [97, 98, 99] as Array<Number>;
            checkEqual("sha256_abc", IqVectors.SHA256_ABC, BridgeCrypto.toHex(sha256(abc)));
            checkEqual("sha256_finish_abc", IqVectors.SHA256_ABC, BridgeCrypto.toHex(BridgeCrypto.sha256Finish(BridgeCrypto.sha256InitialState(), abc, 0)));
        } else if (phase == 2) {
            var multi = new [100] as Array<Number>;
            for (var i = 0; i < 100; i++) { multi[i] = i; }
            checkEqual("sha256_multi_block", IqVectors.SHA256_MULTI_BLOCK, BridgeCrypto.toHex(sha256(multi)));
        } else if (phase == 3) {
            var shortKey = [107, 101, 121] as Array<Number>;
            _inner = BridgeCrypto.hmacPadState(shortKey, 0x36);
            _outer = BridgeCrypto.hmacPadState(shortKey, 0x5c);
            check("hmac_states", _inner.size() == 8 && _outer.size() == 8);
        } else if (phase == 4) {
            checkEqual("hmac_short_key", IqVectors.HMAC_SHORT_KEY, hmacWithStates());
        } else if (phase == 5) {
            checkEqual("hmac_states_reusable", IqVectors.HMAC_SHORT_KEY, hmacWithStates());
        } else if (phase == 6) {
            var sbox = BridgeCrypto.aesSbox();
            check("oversized_hmac_key_rejected", BridgeCrypto.hmacPadState(filled(65, 0xaa), 0x36).size() == 0);
            check("oversized_final_block_rejected", BridgeCrypto.sha256Finish(BridgeCrypto.sha256InitialState(), filled(56, 0), 0).size() == 0);
            check("short_aes_key_rejected", BridgeCrypto.aesRoundKeys(filled(31, 0), sbox).size() == 0);
            _roundKeys = BridgeCrypto.aesRoundKeys(BridgeCrypto.fromHex(IqVectors.AES_KEY, 32), sbox);
            check("aes_round_keys", _roundKeys.size() == 240);
            check("short_aes_block_rejected", BridgeCrypto.aesCbcBlock(filled(15, 0), 0, filled(16, 0), _roundKeys, sbox).size() == 0);
            // The Rust oracle applies PKCS#7: 32 bytes of plaintext gain a full padding block.
            _aesData = BridgeCrypto.concat(BridgeCrypto.fromHex(IqVectors.AES_PLAINTEXT, 32), filled(16, 16));
            _aesPrevious = BridgeCrypto.fromHex(IqVectors.AES_IV, 16);
        } else if (phase == 7) {
            aesStep(0);
        } else if (phase == 8) {
            aesStep(16);
        } else if (phase == 9) {
            aesStep(32);
            checkEqual("aes_256_cbc_three_blocks", IqVectors.AES_CIPHERTEXT, BridgeCrypto.toHex(_aesOut));
        } else if (phase == 10) {
            var engine = new HeartbeatEngine(BridgeCrypto.fromHex(IqVectors.KEY, 32));
            check("engine_not_ready_before_preparation", !engine.begin(1, 1, filled(16, 0)));
            _engine = engine;
        } else if (phase == 11) {
            // One engine slice per tick, exactly as PhoneLink drives it.
            var preparing = _engine as HeartbeatEngine;
            _engineSteps += 1;
            if (preparing.step()) { _phase = phase; return; }
            check("engine_prepared_in_four_slices", _engineSteps == 4 && preparing.isReady() && preparing.error() == null);
            check("invalid_challenge_rejected", !preparing.begin(1, 1, filled(15, 0)));
            check("heartbeat_started", preparing.begin(0x12345678, 0x65abcdef, BridgeCrypto.fromHex(IqVectors.CHALLENGE, 16)));
            _engineSteps = 0;
        } else if (phase == 12) {
            var building = _engine as HeartbeatEngine;
            _engineSteps += 1;
            if (building.step()) { _phase = phase; return; }
            var heartbeat = building.reply();
            check("heartbeat_built_in_six_slices", _engineSteps == 6 && building.error() == null);
            check("heartbeat_length", heartbeat.size() == 81);
            checkEqual("full_heartbeat", IqVectors.HEARTBEAT, BridgeCrypto.toHex(heartbeat));
        } else {
            var report = "ALL_CRYPTO_TESTS_PASSED checks=" + _checks;
            if (_failures.size() > 0) {
                report = "CRYPTO_TESTS_FAILED count=" + _failures.size() + " details=" + _failures;
            }
            finish(report);
        }
    }

    private function finish(report as String) as Void {
        _timer.stop();
        Communications.makeWebRequest(RESULT_URL, { "result" => report }, null, method(:onReportResponse));
    }

    function onReportResponse(responseCode as Number, data as Dictionary or String or Null) as Void {
        System.println("Crypto test report HTTP response: " + responseCode);
    }
}
