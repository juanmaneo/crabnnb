import Toybox.Communications;
import Toybox.Lang;
import Toybox.System;

class BridgeCryptoTest {
    private const RESULT_URL = "http://127.0.0.1:__CIQ_TEST_PORT__/";
    private var _failures as Array<String> = [];
    private var _checks as Number = 0;

    private function check(name as String, passed as Boolean) as Void {
        _checks += 1;
        if (!passed) { _failures.add(name); }
    }

    private function checkEqual(name as String, expected as String, actual as String) as Void {
        check(name, expected.equals(actual));
    }

    private function sendReport(report as String) as Void {
        Communications.makeWebRequest(
            RESULT_URL,
            { "result" => report },
            null,
            method(:onReportResponse)
        );
    }

    function onReportResponse(responseCode as Number, data as Dictionary or String or Null) as Void {
        System.println("Crypto test report HTTP response: " + responseCode);
    }

    function run() as Void {
        var multi = BridgeCrypto.zeros(100);
        for (var i = 0; i < 100; i++) { multi[i] = i; }
        checkEqual("sha256_empty", IqVectors.SHA256_EMPTY, BridgeCrypto.toHex(BridgeCrypto.sha256(BridgeCrypto.zeros(0))));
        checkEqual("sha256_abc", IqVectors.SHA256_ABC, BridgeCrypto.toHex(BridgeCrypto.sha256([97, 98, 99])));
        checkEqual("sha256_multi_block", IqVectors.SHA256_MULTI_BLOCK, BridgeCrypto.toHex(BridgeCrypto.sha256(multi)));

        var message = [84,104,101,32,113,117,105,99,107,32,98,114,111,119,110,32,102,111,120,32,106,117,109,112,115,32,111,118,101,114,32,116,104,101,32,108,97,122,121,32,100,111,103];
        var shortKey = [107,101,121];
        var shortStates = BridgeCrypto.hmacStates(shortKey);
        checkEqual("hmac_short_key", IqVectors.HMAC_SHORT_KEY, BridgeCrypto.toHex(BridgeCrypto.hmacSha256(shortKey, message)));
        checkEqual("hmac_precomputed_short_key", IqVectors.HMAC_SHORT_KEY, BridgeCrypto.toHex(BridgeCrypto.hmacSha256Prepared(shortStates[0], shortStates[1], message)));
        checkEqual("hmac_precomputed_repeat", IqVectors.HMAC_SHORT_KEY, BridgeCrypto.toHex(BridgeCrypto.hmacSha256Prepared(shortStates[0], shortStates[1], message)));
        var longKey = BridgeCrypto.zeros(131);
        for (var j = 0; j < longKey.size(); j++) { longKey[j] = 0xaa; }
        checkEqual("hmac_long_key", IqVectors.HMAC_LONG_KEY, BridgeCrypto.toHex(BridgeCrypto.hmacSha256(longKey, message)));
        var longStates = BridgeCrypto.hmacStates(longKey);
        checkEqual("hmac_precomputed_long_key", IqVectors.HMAC_LONG_KEY, BridgeCrypto.toHex(BridgeCrypto.hmacSha256Prepared(longStates[0], longStates[1], message)));

        var aesKey = BridgeCrypto.fromHex(IqVectors.AES_KEY, 32) as Array<Number>;
        var aesIv = BridgeCrypto.fromHex(IqVectors.AES_IV, 16) as Array<Number>;
        var aesPlaintext = BridgeCrypto.fromHex(IqVectors.AES_PLAINTEXT, 32) as Array<Number>;
        checkEqual("aes_256_cbc_two_blocks", IqVectors.AES_CIPHERTEXT, BridgeCrypto.toHex(BridgeCrypto.aes256CbcEncrypt(aesKey, aesIv, aesPlaintext)));

        var key = BridgeCrypto.fromHex(IqVectors.KEY, 32) as Array<Number>;
        var challenge = BridgeCrypto.fromHex(IqVectors.CHALLENGE, 16) as Array<Number>;
        var preparedKey = BridgeCrypto.prepareKey(key) as Array<Array<Number>>;
        var heartbeat = BridgeCrypto.buildHeartbeat(preparedKey, 0x12345678, 0x65abcdef, challenge);
        check("heartbeat_length", heartbeat.size() == 81);
        checkEqual("full_heartbeat", IqVectors.HEARTBEAT, BridgeCrypto.toHex(heartbeat));
        check("invalid_key_rejected", BridgeCrypto.prepareKey(BridgeCrypto.zeros(31)) == null);
        check("invalid_challenge_rejected", BridgeCrypto.buildHeartbeat(preparedKey, 1, 1, BridgeCrypto.zeros(15)).size() == 0);
        checkEqual("hex_parsing", "00a1ff", BridgeCrypto.toHex(BridgeCrypto.fromHex("00A1ff", 3) as Array<Number>));
        check("malformed_hex_rejected", BridgeCrypto.fromHex("abc", 2) == null && BridgeCrypto.fromHex("00xz", 2) == null);

        var report = "ALL_CRYPTO_TESTS_PASSED checks=" + _checks;
        if (_failures.size() > 0) {
            report = "CRYPTO_TESTS_FAILED count=" + _failures.size() + " details=" + _failures;
        }
        sendReport(report);
    }
}
