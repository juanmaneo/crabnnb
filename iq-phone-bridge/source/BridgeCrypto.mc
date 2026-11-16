// BridgeCrypto: cryptographic heartbeat builder for the phone-bridge transport.
//
// Monkey C's Toybox.Cryptography offers AES and SHA-256 but no AEAD, so the
// bridge message uses encrypt-then-MAC:
//
//   msg  = 0x01 || iv(16) || AES-256-CBC(PKCS7, pt)(32) || HMAC-SHA256(macKey, msg[0..49])(32)
//   pt   = counter u32le(4) || unix-seconds u32le(4) || challenge(16)   (24 bytes)
//   macKey = SHA256("crabnnb-mac" || key)
//   iv   = HMAC-SHA256(macKey, pt)[0..16]   (unique per message, no RNG needed)
//
// This must match crabnnb's src/heartbeat.rs (MODE_AES_CBC_HMAC) byte for byte.

import Toybox.Cryptography;
import Toybox.Lang;

class BridgeCrypto {

    static function hexToBytes(hex as String) as ByteArray {
        var clean = "";
        for (var i = 0; i < hex.length(); i++) {
            var c = hex.substring(i, i + 1);
            if (c != " " && c != "\n" && c != "\t" && c != "\r") {
                clean += c;
            }
        }
        if (clean.length() != 64) {
            return null;
        }
        var out = new ByteArray(32);
        for (var j = 0; j < 32; j++) {
            out[j] = hexVal(clean.substring(j * 2, j * 2 + 1)) * 16
                   + hexVal(clean.substring(j * 2 + 1, j * 2 + 2));
        }
        return out;
    }

    static function hexVal(s as String) as Number {
        var c = s.charCodeAt(0);
        if (c >= 48 && c <= 57) { return c - 48; }        // '0'..'9'
        var u = s.toUpperCase().charCodeAt(0);
        if (u >= 65 && u <= 70) { return u - 65 + 10; }    // 'A'..'F'
        return -1;
    }

    static function macKey(key as ByteArray) as ByteArray {
        var tag = "crabnnb-mac";
        var buf = new ByteArray(tag.length() + key.size());
        for (var i = 0; i < tag.length(); i++) {
            buf[i] = tag.charCodeAt(i);
        }
        for (var j = 0; j < key.size(); j++) {
            buf[tag.length() + j] = key[j];
        }
        return Digest.hash(buf, Digest.SHA256);
    }

    static function hmacSha256(key as ByteArray, msg as ByteArray) as ByteArray {
        var block = 64;
        var k = key;
        if (k.size() > block) {
            k = Digest.hash(k, Digest.SHA256);
        }
        var kp = new ByteArray(block);
        for (var i = 0; i < block; i++) {
            kp[i] = (i < k.size()) ? k[i] : 0;
        }
        var ipad = new ByteArray(block);
        var opad = new ByteArray(block);
        for (var j = 0; j < block; j++) {
            ipad[j] = kp[j] ^ 0x36;
            opad[j] = kp[j] ^ 0x5c;
        }
        var inner = Digest.hash(concat(ipad, msg), Digest.SHA256);
        return Digest.hash(concat(opad, inner), Digest.SHA256);
    }

    static function concat(a as ByteArray, b as ByteArray) as ByteArray {
        var out = new ByteArray(a.size() + b.size());
        for (var i = 0; i < a.size(); i++) {
            out[i] = a[i];
        }
        for (var j = 0; j < b.size(); j++) {
            out[a.size() + j] = b[j];
        }
        return out;
    }

    static function copyInto(dst as ByteArray, offset as Number, src as ByteArray) as Void {
        for (var i = 0; i < src.size(); i++) {
            dst[offset + i] = src[i];
        }
    }

    static function u32le(value as Number) as ByteArray {
        var b = new ByteArray(4);
        b[0] = value & 0xFF;
        b[1] = (value >> 8) & 0xFF;
        b[2] = (value >> 16) & 0xFF;
        b[3] = (value >> 24) & 0xFF;
        return b;
    }

    static function pkcs7Pad(data as ByteArray) as ByteArray {
        var pad = 16 - (data.size() % 16);
        var out = new ByteArray(data.size() + pad);
        for (var i = 0; i < data.size(); i++) {
            out[i] = data[i];
        }
        for (var j = data.size(); j < out.size(); j++) {
            out[j] = pad;
        }
        return out;
    }

    static function buildHeartbeat(key as ByteArray, counter as Number, tsSec as Number, challenge as ByteArray) as ByteArray {
        var pt = new ByteArray(24);
        copyInto(pt, 0, u32le(counter));
        copyInto(pt, 4, u32le(tsSec));
        copyInto(pt, 8, challenge);

        var mk = macKey(key);
        var ivFull = hmacSha256(mk, pt);
        var iv = new ByteArray(16);
        copyInto(iv, 0, subarray(ivFull, 0, 16));

        var cipher = new Cipher(Cipher.ENCRYPTION_AES_256_CBC, key, iv);
        var ct = cipher.encrypt(pkcs7Pad(pt));

        var msg = new ByteArray(1 + 16 + ct.size() + 32);
        msg[0] = 0x01;
        copyInto(msg, 1, iv);
        copyInto(msg, 17, ct);
        var mac = hmacSha256(mk, subarray(msg, 0, 1 + 16 + ct.size()));
        copyInto(msg, 1 + 16 + ct.size(), mac);
        return msg;
    }

    static function subarray(src as ByteArray, start as Number, end as Number) as ByteArray {
        var out = new ByteArray(end - start);
        for (var i = start; i < end; i++) {
            out[i - start] = src[i];
        }
        return out;
    }
}
