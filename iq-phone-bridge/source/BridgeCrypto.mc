// API 1.4-compatible bridge cryptography using Array<Number> byte buffers.
// This intentionally avoids Lang.ByteArray and Toybox.Cryptography (API 3.0+).
// Wire format must match src/heartbeat.rs, MODE_AES_CBC_HMAC.
//
// msg = 0x01 || iv(16) || AES-256-CBC-PKCS7(plaintext)(32) || HMAC-SHA256(macKey, first 49 bytes)(32)
// plaintext = counter u32le || unix-seconds u32le || challenge(16)
// macKey = SHA256("crabnnb-mac" || key), iv = HMAC-SHA256(macKey, plaintext)[0..16]
//
// The FR235 watchdog kills an app (and can reboot the watch) when one callback
// executes too long, and that cannot be caught. Each function here is therefore
// bounded to at most one SHA-256 compression or one AES block, with the arithmetic
// inlined; HeartbeatEngine spreads the calls across timer ticks.
// Invalid input is reported by returning an empty array, never by throwing.

import Toybox.Lang;

class BridgeCrypto {
    private static var _sha256K as Array<Number>? = null;
    private static var _aesSbox as Array<Number>? = null;

    static function toHex(bytes as Array<Number>) as String {
        var digits = ["0", "1", "2", "3", "4", "5", "6", "7", "8", "9", "a", "b", "c", "d", "e", "f"];
        var out = "";
        for (var i = 0; i < bytes.size(); i++) {
            var b = bytes[i];
            out = out + digits[(b >> 4) & 15] + digits[b & 15];
        }
        return out;
    }

    static function hexValue(c as Number) as Number {
        if (c >= 48 && c <= 57) { return c - 48; }
        if (c >= 97 && c <= 102) { return c - 87; }
        if (c >= 65 && c <= 70) { return c - 55; }
        return -1;
    }

    static function fromHex(value as String, expectedBytes as Number) as Array<Number> {
        var empty = [] as Array<Number>;
        if (value.length() != expectedBytes * 2) { return empty; }
        var chars = value.toCharArray();
        if (chars.size() != expectedBytes * 2) { return empty; }
        var out = new [expectedBytes] as Array<Number>;
        for (var i = 0; i < expectedBytes; i++) {
            var high = hexValue(chars[i * 2].toNumber());
            var low = hexValue(chars[i * 2 + 1].toNumber());
            if (high < 0 || low < 0) { return empty; }
            out[i] = high * 16 + low;
        }
        return out;
    }

    static function concat(a as Array<Number>, b as Array<Number>) as Array<Number> {
        var first = a.size();
        var second = b.size();
        var out = new [first + second] as Array<Number>;
        for (var i = 0; i < first; i++) { out[i] = a[i]; }
        for (var j = 0; j < second; j++) { out[first + j] = b[j]; }
        return out;
    }

    static function slice(src as Array<Number>, start as Number, end as Number) as Array<Number> {
        if (start < 0 || end < start || end > src.size()) { return [] as Array<Number>; }
        var out = new [end - start] as Array<Number>;
        for (var i = start; i < end; i++) { out[i - start] = src[i]; }
        return out;
    }

    static function sha256Constants() as Array<Number> {
        var k = _sha256K;
        if (k == null) {
            k = [
                0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
                0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
                0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
                0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
                0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
                0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
                0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
                0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2
            ] as Array<Number>;
            _sha256K = k;
        }
        return k;
    }

    static function sha256InitialState() as Array<Number> {
        return [0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19] as Array<Number>;
    }

    // Absorbs the 64-byte block at data[block..block+64] into the 8-word state `hash`.
    // Number is a wrapping signed 32-bit integer; `>>` may sign-extend, so every right
    // shift is masked to make it logical. Callers guarantee the array sizes.
    static function sha256Compress(hash as Array<Number>, data as Array<Number>, block as Number) as Void {
        var k = sha256Constants();
        var w = new [64] as Array<Number>;
        var p = block;
        for (var j = 0; j < 16; j++) {
            w[j] = (data[p] << 24) | (data[p + 1] << 16) | (data[p + 2] << 8) | data[p + 3];
            p += 4;
        }
        for (var i = 16; i < 64; i++) {
            var x = w[i - 15];
            var y = w[i - 2];
            w[i] = w[i - 16]
                + ((((x >> 7) & 0x01ffffff) | (x << 25)) ^ (((x >> 18) & 0x00003fff) | (x << 14)) ^ ((x >> 3) & 0x1fffffff))
                + w[i - 7]
                + ((((y >> 17) & 0x00007fff) | (y << 15)) ^ (((y >> 19) & 0x00001fff) | (y << 13)) ^ ((y >> 10) & 0x003fffff));
        }

        var a = hash[0]; var b = hash[1]; var c = hash[2]; var d = hash[3];
        var e = hash[4]; var f = hash[5]; var g = hash[6]; var h = hash[7];
        for (var r = 0; r < 64; r++) {
            var t1 = h
                + ((((e >> 6) & 0x03ffffff) | (e << 26)) ^ (((e >> 11) & 0x001fffff) | (e << 21)) ^ (((e >> 25) & 0x0000007f) | (e << 7)))
                + ((e & f) ^ ((~e) & g))
                + k[r] + w[r];
            var t2 = ((((a >> 2) & 0x3fffffff) | (a << 30)) ^ (((a >> 13) & 0x0007ffff) | (a << 19)) ^ (((a >> 22) & 0x000003ff) | (a << 10)))
                + ((a & b) ^ (a & c) ^ (b & c));
            h = g; g = f; f = e; e = d + t1;
            d = c; c = b; b = a; a = t1 + t2;
        }
        hash[0] = hash[0] + a; hash[1] = hash[1] + b;
        hash[2] = hash[2] + c; hash[3] = hash[3] + d;
        hash[4] = hash[4] + e; hash[5] = hash[5] + f;
        hash[6] = hash[6] + g; hash[7] = hash[7] + h;
    }

    static function sha256Bytes(hash as Array<Number>) as Array<Number> {
        var output = new [32] as Array<Number>;
        for (var n = 0; n < 8; n++) {
            var word = hash[n];
            output[n * 4] = (word >> 24) & 0xff;
            output[n * 4 + 1] = (word >> 16) & 0xff;
            output[n * 4 + 2] = (word >> 8) & 0xff;
            output[n * 4 + 3] = word & 0xff;
        }
        return output;
    }

    // Finishes SHA-256 with one compression: `state` is the chaining value after
    // `priorBytes` already-absorbed bytes, and `message` (<= 55 bytes) is the rest.
    // `state` is not modified, so a precomputed state can be reused.
    static function sha256Finish(state as Array<Number>, message as Array<Number>, priorBytes as Number) as Array<Number> {
        var size = message.size();
        if (state.size() != 8 || size > 55) { return [] as Array<Number>; }
        var data = new [64] as Array<Number>;
        for (var i = 0; i < size; i++) { data[i] = message[i]; }
        data[size] = 0x80;
        for (var j = size + 1; j < 62; j++) { data[j] = 0; }
        var bits = (priorBytes + size) * 8;
        data[62] = (bits >> 8) & 0xff;
        data[63] = bits & 0xff;
        var hash = new [8] as Array<Number>;
        for (var n = 0; n < 8; n++) { hash[n] = state[n]; }
        sha256Compress(hash, data, 0);
        return sha256Bytes(hash);
    }

    // SHA-256 state after absorbing the 64-byte HMAC pad block (key ^ pad), where
    // pad is 0x36 (inner) or 0x5c (outer). HMAC(key, m) is then
    // sha256Finish(outer, sha256Finish(inner, m, 64), 64) for m <= 55 bytes.
    static function hmacPadState(key as Array<Number>, pad as Number) as Array<Number> {
        var size = key.size();
        if (size > 64) { return [] as Array<Number>; }
        var data = new [64] as Array<Number>;
        for (var i = 0; i < 64; i++) { data[i] = (i < size) ? (key[i] ^ pad) : pad; }
        var state = sha256InitialState();
        sha256Compress(state, data, 0);
        return state;
    }

    static function aesSbox() as Array<Number> {
        var sbox = _aesSbox;
        if (sbox == null) {
            sbox = [
                0x63,0x7c,0x77,0x7b,0xf2,0x6b,0x6f,0xc5,0x30,0x01,0x67,0x2b,0xfe,0xd7,0xab,0x76,
                0xca,0x82,0xc9,0x7d,0xfa,0x59,0x47,0xf0,0xad,0xd4,0xa2,0xaf,0x9c,0xa4,0x72,0xc0,
                0xb7,0xfd,0x93,0x26,0x36,0x3f,0xf7,0xcc,0x34,0xa5,0xe5,0xf1,0x71,0xd8,0x31,0x15,
                0x04,0xc7,0x23,0xc3,0x18,0x96,0x05,0x9a,0x07,0x12,0x80,0xe2,0xeb,0x27,0xb2,0x75,
                0x09,0x83,0x2c,0x1a,0x1b,0x6e,0x5a,0xa0,0x52,0x3b,0xd6,0xb3,0x29,0xe3,0x2f,0x84,
                0x53,0xd1,0x00,0xed,0x20,0xfc,0xb1,0x5b,0x6a,0xcb,0xbe,0x39,0x4a,0x4c,0x58,0xcf,
                0xd0,0xef,0xaa,0xfb,0x43,0x4d,0x33,0x85,0x45,0xf9,0x02,0x7f,0x50,0x3c,0x9f,0xa8,
                0x51,0xa3,0x40,0x8f,0x92,0x9d,0x38,0xf5,0xbc,0xb6,0xda,0x21,0x10,0xff,0xf3,0xd2,
                0xcd,0x0c,0x13,0xec,0x5f,0x97,0x44,0x17,0xc4,0xa7,0x7e,0x3d,0x64,0x5d,0x19,0x73,
                0x60,0x81,0x4f,0xdc,0x22,0x2a,0x90,0x88,0x46,0xee,0xb8,0x14,0xde,0x5e,0x0b,0xdb,
                0xe0,0x32,0x3a,0x0a,0x49,0x06,0x24,0x5c,0xc2,0xd3,0xac,0x62,0x91,0x95,0xe4,0x79,
                0xe7,0xc8,0x37,0x6d,0x8d,0xd5,0x4e,0xa9,0x6c,0x56,0xf4,0xea,0x65,0x7a,0xae,0x08,
                0xba,0x78,0x25,0x2e,0x1c,0xa6,0xb4,0xc6,0xe8,0xdd,0x74,0x1f,0x4b,0xbd,0x8b,0x8a,
                0x70,0x3e,0xb5,0x66,0x48,0x03,0xf6,0x0e,0x61,0x35,0x57,0xb9,0x86,0xc1,0x1d,0x9e,
                0xe1,0xf8,0x98,0x11,0x69,0xd9,0x8e,0x94,0x9b,0x1e,0x87,0xe9,0xce,0x55,0x28,0xdf,
                0x8c,0xa1,0x89,0x0d,0xbf,0xe6,0x42,0x68,0x41,0x99,0x2d,0x0f,0xb0,0x54,0xbb,0x16
            ] as Array<Number>;
            _aesSbox = sbox;
        }
        return sbox;
    }

    // AES-256 key schedule: 15 round keys, 240 bytes.
    static function aesRoundKeys(key as Array<Number>, sbox as Array<Number>) as Array<Number> {
        if (key.size() != 32 || sbox.size() != 256) { return [] as Array<Number>; }
        var expanded = new [240] as Array<Number>;
        for (var i = 0; i < 32; i++) { expanded[i] = key[i]; }
        var rcon = 1;
        for (var pos = 32; pos < 240; pos += 4) {
            var t0 = expanded[pos - 4]; var t1 = expanded[pos - 3];
            var t2 = expanded[pos - 2]; var t3 = expanded[pos - 1];
            var phase = pos % 32;
            if (phase == 0) {
                var first = t0;
                t0 = sbox[t1] ^ rcon; t1 = sbox[t2]; t2 = sbox[t3]; t3 = sbox[first];
                rcon = rcon << 1;
            } else if (phase == 16) {
                t0 = sbox[t0]; t1 = sbox[t1]; t2 = sbox[t2]; t3 = sbox[t3];
            }
            expanded[pos] = expanded[pos - 32] ^ t0;
            expanded[pos + 1] = expanded[pos - 31] ^ t1;
            expanded[pos + 2] = expanded[pos - 30] ^ t2;
            expanded[pos + 3] = expanded[pos - 29] ^ t3;
        }
        return expanded;
    }

    // One AES-256-CBC block: encrypts data[offset..offset+16] XOR `previous` (the IV
    // or the preceding ciphertext block). SubBytes and ShiftRows are merged through
    // an index table, and MixColumns and AddRoundKey are merged per column.
    static function aesCbcBlock(data as Array<Number>, offset as Number, previous as Array<Number>,
                                roundKeys as Array<Number>, sbox as Array<Number>) as Array<Number> {
        if (offset < 0 || data.size() < offset + 16 || previous.size() != 16
                || roundKeys.size() != 240 || sbox.size() != 256) {
            return [] as Array<Number>;
        }
        var shift = [0, 5, 10, 15, 4, 9, 14, 3, 8, 13, 2, 7, 12, 1, 6, 11];
        var s = new [16] as Array<Number>;
        var t = new [16] as Array<Number>;
        for (var i = 0; i < 16; i++) { s[i] = data[offset + i] ^ previous[i] ^ roundKeys[i]; }
        var rk = 16;
        for (var round = 1; round < 14; round++) {
            for (var j = 0; j < 16; j++) { t[j] = sbox[s[shift[j]]]; }
            for (var c = 0; c < 16; c += 4) {
                var a0 = t[c]; var a1 = t[c + 1]; var a2 = t[c + 2]; var a3 = t[c + 3];
                var all = a0 ^ a1 ^ a2 ^ a3;
                var x = a0 ^ a1;
                s[c] = a0 ^ all ^ ((x << 1) & 0xff) ^ ((x >> 7) * 0x1b) ^ roundKeys[rk + c];
                x = a1 ^ a2;
                s[c + 1] = a1 ^ all ^ ((x << 1) & 0xff) ^ ((x >> 7) * 0x1b) ^ roundKeys[rk + c + 1];
                x = a2 ^ a3;
                s[c + 2] = a2 ^ all ^ ((x << 1) & 0xff) ^ ((x >> 7) * 0x1b) ^ roundKeys[rk + c + 2];
                x = a3 ^ a0;
                s[c + 3] = a3 ^ all ^ ((x << 1) & 0xff) ^ ((x >> 7) * 0x1b) ^ roundKeys[rk + c + 3];
            }
            rk += 16;
        }
        for (var n = 0; n < 16; n++) { t[n] = sbox[s[shift[n]]] ^ roundKeys[224 + n]; }
        return t;
    }
}
