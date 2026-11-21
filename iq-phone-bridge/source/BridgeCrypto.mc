// API 1.4-compatible bridge cryptography using Array<Number> byte buffers.
// This intentionally avoids Lang.ByteArray and Toybox.Cryptography (API 3.0+).
// Wire format must match src/heartbeat.rs, MODE_AES_CBC_HMAC.
//
// msg = 0x01 || iv(16) || AES-256-CBC-PKCS7(plaintext)(32) || HMAC-SHA256(macKey, first 49 bytes)(32)
// plaintext = counter u32le || unix-seconds u32le || challenge(16)
// macKey = SHA256("crabnnb-mac" || key), iv = HMAC-SHA256(macKey, plaintext)[0..16]

import Toybox.Lang;

class BridgeCrypto {
    static function zeros(size as Number) as Array<Number> {
        var result = new [size] as Array<Number>;
        for (var i = 0; i < size; i++) { result[i] = 0; }
        return result;
    }

    static function hexDigit(value as Number) as String {
        var alphabet = "0123456789abcdef";
        return alphabet.substring(value & 15, (value & 15) + 1) as String;
    }

    static function toHex(bytes as Array<Number>) as String {
        var out = "";
        for (var i = 0; i < bytes.size(); i++) {
            out += hexDigit(bytes[i] >> 4) + hexDigit(bytes[i]);
        }
        return out;
    }

    static function hexValue(value as String) as Number {
        if (value.length() != 1) { return -1; }
        var c = value.toUpper().toCharArray()[0].toNumber();
        if (c >= 48 && c <= 57) { return c - 48; }
        if (c >= 65 && c <= 70) { return c - 55; }
        return -1;
    }

    static function fromHex(value as String, expectedBytes as Number) as Array<Number>? {
        if (value.length() != expectedBytes * 2) { return null; }
        var out = zeros(expectedBytes);
        for (var i = 0; i < expectedBytes; i++) {
            var high = hexValue(value.substring(i * 2, i * 2 + 1) as String);
            var low = hexValue(value.substring(i * 2 + 1, i * 2 + 2) as String);
            if (high < 0 || low < 0) { return null; }
            out[i] = high * 16 + low;
        }
        return out;
    }

    static function concat(a as Array<Number>, b as Array<Number>) as Array<Number> {
        var out = zeros(a.size() + b.size());
        copyInto(out, 0, a);
        copyInto(out, a.size(), b);
        return out;
    }

    static function copyInto(dst as Array<Number>, offset as Number, src as Array<Number>) as Void {
        for (var i = 0; i < src.size(); i++) { dst[offset + i] = src[i]; }
    }

    static function slice(src as Array<Number>, start as Number, end as Number) as Array<Number> {
        var out = zeros(end - start);
        for (var i = start; i < end; i++) { out[i - start] = src[i]; }
        return out;
    }

    static function u32le(value as Number) as Array<Number> {
        return [value & 0xff, (value >> 8) & 0xff, (value >> 16) & 0xff, (value >> 24) & 0xff];
    }

    static function add32(a as Number, b as Number) as Number {
        return (a + b) & 0xffffffff;
    }

    static function addMany(a as Number, b as Number, c as Number, d as Number, e as Number) as Number {
        return add32(add32(add32(add32(a, b), c), d), e);
    }

    static function logicalRight(value as Number, count as Number) as Number {
        if (count == 0) { return value; }
        return (value >> count) & (0x7fffffff >> (count - 1));
    }

    static function rotateRight(value as Number, count as Number) as Number {
        return logicalRight(value, count) | (value << (32 - count));
    }

    static function sha256Constants() as Array<Number> {
        return [
            0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
            0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
            0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
            0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
            0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
            0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
            0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
            0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2
        ];
    }

    static function sha256(input as Array<Number>) as Array<Number> {
        var paddedSize = ((input.size() + 9 + 63) / 64).toNumber() * 64;
        var data = zeros(paddedSize);
        copyInto(data, 0, input);
        data[input.size()] = 0x80;
        var bitLength = input.size() * 8;
        for (var i = 0; i < 4; i++) {
            data[paddedSize - 1 - i] = (bitLength >> (i * 8)) & 0xff;
        }

        var hash = [0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19];
        var constants = sha256Constants();
        for (var block = 0; block < paddedSize; block += 64) {
            var words = zeros(64);
            for (var j = 0; j < 16; j++) {
                var p = block + j * 4;
                words[j] = ((data[p] & 0xff) << 24) | ((data[p + 1] & 0xff) << 16) | ((data[p + 2] & 0xff) << 8) | (data[p + 3] & 0xff);
            }
            for (var k = 16; k < 64; k++) {
                var x = words[k - 15];
                var y = words[k - 2];
                var s0 = rotateRight(x, 7) ^ rotateRight(x, 18) ^ logicalRight(x, 3);
                var s1 = rotateRight(y, 17) ^ rotateRight(y, 19) ^ logicalRight(y, 10);
                words[k] = add32(add32(add32(words[k - 16], s0), words[k - 7]), s1);
            }

            var a = hash[0]; var b = hash[1]; var c = hash[2]; var d = hash[3];
            var e = hash[4]; var f = hash[5]; var g = hash[6]; var h = hash[7];
            for (var round = 0; round < 64; round++) {
                var big1 = rotateRight(e, 6) ^ rotateRight(e, 11) ^ rotateRight(e, 25);
                var choose = (e & f) ^ ((~e) & g);
                var t1 = addMany(h, big1, choose, constants[round], words[round]);
                var big0 = rotateRight(a, 2) ^ rotateRight(a, 13) ^ rotateRight(a, 22);
                var majority = (a & b) ^ (a & c) ^ (b & c);
                var t2 = add32(big0, majority);
                h = g; g = f; f = e; e = add32(d, t1);
                d = c; c = b; b = a; a = add32(t1, t2);
            }
            hash[0] = add32(hash[0], a); hash[1] = add32(hash[1], b);
            hash[2] = add32(hash[2], c); hash[3] = add32(hash[3], d);
            hash[4] = add32(hash[4], e); hash[5] = add32(hash[5], f);
            hash[6] = add32(hash[6], g); hash[7] = add32(hash[7], h);
        }

        var output = zeros(32);
        for (var n = 0; n < 8; n++) {
            output[n * 4] = (hash[n] >> 24) & 0xff;
            output[n * 4 + 1] = (hash[n] >> 16) & 0xff;
            output[n * 4 + 2] = (hash[n] >> 8) & 0xff;
            output[n * 4 + 3] = hash[n] & 0xff;
        }
        return output;
    }

    static function hmacSha256(key as Array<Number>, message as Array<Number>) as Array<Number> {
        var normalized = key;
        if (normalized.size() > 64) { normalized = sha256(normalized); }
        var innerPad = zeros(64); var outerPad = zeros(64);
        for (var i = 0; i < 64; i++) {
            var byte = (i < normalized.size()) ? normalized[i] : 0;
            innerPad[i] = byte ^ 0x36;
            outerPad[i] = byte ^ 0x5c;
        }
        return sha256(concat(outerPad, sha256(concat(innerPad, message))));
    }

    static function aesSbox() as Array<Number> {
        return [
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
        ];
    }

    static function aesRoundKeys(key as Array<Number>, sbox as Array<Number>) as Array<Number> {
        var expanded = zeros(240);
        copyInto(expanded, 0, key);
        var rcon = [0,1,2,4,8,16,32,64];
        var generated = 32; var round = 1;
        var temp = zeros(4);
        while (generated < 240) {
            for (var i = 0; i < 4; i++) { temp[i] = expanded[generated - 4 + i]; }
            if ((generated % 32) == 0) {
                var first = temp[0]; temp[0] = sbox[temp[1]] ^ rcon[round];
                temp[1] = sbox[temp[2]]; temp[2] = sbox[temp[3]]; temp[3] = sbox[first];
                round += 1;
            } else if ((generated % 32) == 16) {
                for (var j = 0; j < 4; j++) { temp[j] = sbox[temp[j]]; }
            }
            for (var k = 0; k < 4; k++) {
                expanded[generated] = expanded[generated - 32] ^ temp[k];
                generated += 1;
            }
        }
        return expanded;
    }

    static function xtime(value as Number) as Number {
        var shifted = (value << 1) & 0xff;
        if ((value & 0x80) != 0) { shifted = shifted ^ 0x1b; }
        return shifted;
    }

    static function aesEncryptBlock(input as Array<Number>, roundKeys as Array<Number>, sbox as Array<Number>) as Array<Number> {
        var state = slice(input, 0, 16);
        for (var i = 0; i < 16; i++) { state[i] = state[i] ^ roundKeys[i]; }
        for (var round = 1; round <= 14; round++) {
            for (var j = 0; j < 16; j++) { state[j] = sbox[state[j]]; }
            var shifted = slice(state, 0, 16);
            for (var row = 0; row < 4; row++) {
                for (var col = 0; col < 4; col++) {
                    state[row + 4 * col] = shifted[row + 4 * ((col + row) % 4)];
                }
            }
            if (round < 14) {
                for (var col2 = 0; col2 < 4; col2++) {
                    var p = col2 * 4;
                    var a = state[p]; var b = state[p + 1]; var c = state[p + 2]; var d = state[p + 3];
                    var all = a ^ b ^ c ^ d;
                    state[p] = a ^ all ^ xtime(a ^ b);
                    state[p + 1] = b ^ all ^ xtime(b ^ c);
                    state[p + 2] = c ^ all ^ xtime(c ^ d);
                    state[p + 3] = d ^ all ^ xtime(d ^ a);
                }
            }
            var keyOffset = round * 16;
            for (var n = 0; n < 16; n++) { state[n] = state[n] ^ roundKeys[keyOffset + n]; }
        }
        return state;
    }

    static function aes256CbcEncrypt(key as Array<Number>, iv as Array<Number>, plaintext as Array<Number>) as Array<Number> {
        var pad = 16 - (plaintext.size() % 16);
        var padded = zeros(plaintext.size() + pad);
        copyInto(padded, 0, plaintext);
        for (var i = plaintext.size(); i < padded.size(); i++) { padded[i] = pad; }
        var sbox = aesSbox();
        var keys = aesRoundKeys(key, sbox);
        var previous = slice(iv, 0, 16);
        var ciphertext = zeros(padded.size());
        for (var block = 0; block < padded.size(); block += 16) {
            var current = slice(padded, block, block + 16);
            for (var j = 0; j < 16; j++) { current[j] = current[j] ^ previous[j]; }
            previous = aesEncryptBlock(current, keys, sbox);
            copyInto(ciphertext, block, previous);
        }
        return ciphertext;
    }

    static function macKey(key as Array<Number>) as Array<Number> {
        var tag = [99,114,97,98,110,110,98,45,109,97,99];
        return sha256(concat(tag, key));
    }

    static function buildHeartbeat(key as Array<Number>, counter as Number, tsSec as Number, challenge as Array<Number>) as Array<Number> {
        if (key.size() != 32 || challenge.size() != 16) { return zeros(0); }
        var plaintext = concat(concat(u32le(counter), u32le(tsSec)), challenge);
        var keyMac = macKey(key);
        var iv = slice(hmacSha256(keyMac, plaintext), 0, 16);
        var ciphertext = aes256CbcEncrypt(key, iv, plaintext);
        var authenticated = concat([1], concat(iv, ciphertext));
        return concat(authenticated, hmacSha256(keyMac, authenticated));
    }
}
