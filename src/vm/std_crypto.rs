use ring::digest::{self, SHA256, SHA512};
use ring::hmac;
use ring::rand::{SecureRandom, SystemRandom};

use super::gc::{gc_alloc_string, gc_allocate, get_pooled_vec, GcData};
use super::value::Value;
use super::execute::VM;

fn hex_encode(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push_str(&format!("{:02x}", b));
    }
    s
}

// RFC 1321 MD5 Implementation
fn md5_digest(data: &[u8]) -> [u8; 16] {
    let mut a: u32 = 0x67452301;
    let mut b: u32 = 0xefcdab89;
    let mut c: u32 = 0x98badcfe;
    let mut d: u32 = 0x10325476;

    let orig_len_bits = (data.len() as u64) * 8;
    let mut msg = data.to_vec();
    msg.push(0x80);
    while (msg.len() % 64) != 56 {
        msg.push(0x00);
    }
    msg.extend_from_slice(&orig_len_bits.to_le_bytes());

    let s = [
        7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22,
        5,  9, 14, 20, 5,  9, 14, 20, 5,  9, 14, 20, 5,  9, 14, 20,
        4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23,
        6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21,
    ];

    let k: [u32; 64] = [
        0xd76aa478, 0xe8c7b756, 0x242070db, 0xc1bdceee,
        0xf57c0faf, 0x4787c62a, 0xa8304613, 0xfd469501,
        0x698098d8, 0x8b44f7af, 0xffff5bb1, 0x895cd7be,
        0x6b901122, 0xfd987193, 0xa679438e, 0x49b40821,
        0xf61e2562, 0xc040b340, 0x265e5a51, 0xe9b6c7aa,
        0xd62f105d, 0x02441453, 0xd8a1e681, 0xe7d3fbc8,
        0x21e1cde6, 0xc33707d6, 0xf4d50d87, 0x455a14ed,
        0xa9e3e905, 0xfcefa3f8, 0x676f02d9, 0x8d2a4c8a,
        0xfffa3942, 0x8771f681, 0x6d9d6122, 0xfde5380c,
        0xa4beea44, 0x4bdecfa9, 0xf6bb4b60, 0xbebfbc70,
        0x289b7ec6, 0xeaa127fa, 0xd4ef3085, 0x04881d05,
        0xd9d4d039, 0xe6db99e5, 0x1fa27cf8, 0xc4ac5665,
        0xf4292244, 0x432aff97, 0xab9423a7, 0xfc93a039,
        0x655b59c3, 0x8f0ccc92, 0xffeff47d, 0x85845dd1,
        0x6fa87e4f, 0xfe2ce6e0, 0xa3014314, 0x4e0811a1,
        0xf7537e82, 0xbd3af235, 0x2ad7d2bb, 0xeb86d391,
    ];

    for chunk in msg.chunks(64) {
        let mut m = [0u32; 16];
        for i in 0..16 {
            m[i] = u32::from_le_bytes(chunk[i * 4..(i + 1) * 4].try_into().unwrap());
        }

        let mut aa = a;
        let mut bb = b;
        let mut cc = c;
        let mut dd = d;

        for i in 0..64 {
            let (f, g) = match i {
                0..=15 => ((bb & cc) | ((!bb) & dd), i),
                16..=31 => ((dd & bb) | ((!dd) & cc), (5 * i + 1) % 16),
                32..=47 => (bb ^ cc ^ dd, (3 * i + 5) % 16),
                _ => (cc ^ (bb | (!dd)), (7 * i) % 16),
            };

            let temp = dd;
            dd = cc;
            cc = bb;
            let sum = aa.wrapping_add(f).wrapping_add(k[i]).wrapping_add(m[g]);
            bb = bb.wrapping_add(sum.rotate_left(s[i]));
            aa = temp;
        }

        a = a.wrapping_add(aa);
        b = b.wrapping_add(bb);
        c = c.wrapping_add(cc);
        d = d.wrapping_add(dd);
    }

    let mut result = [0u8; 16];
    result[0..4].copy_from_slice(&a.to_le_bytes());
    result[4..8].copy_from_slice(&b.to_le_bytes());
    result[8..12].copy_from_slice(&c.to_le_bytes());
    result[12..16].copy_from_slice(&d.to_le_bytes());
    result
}

fn hmac_md5(key: &[u8], data: &[u8]) -> [u8; 16] {
    let mut key_block = [0u8; 64];
    if key.len() > 64 {
        let hash = md5_digest(key);
        key_block[..16].copy_from_slice(&hash);
    } else {
        key_block[..key.len()].copy_from_slice(key);
    }

    let mut o_key_pad = [0u8; 64];
    let mut i_key_pad = [0u8; 64];
    for i in 0..64 {
        o_key_pad[i] = key_block[i] ^ 0x5c;
        i_key_pad[i] = key_block[i] ^ 0x36;
    }

    let mut inner_data = Vec::with_capacity(64 + data.len());
    inner_data.extend_from_slice(&i_key_pad);
    inner_data.extend_from_slice(data);
    let inner_hash = md5_digest(&inner_data);

    let mut outer_data = Vec::with_capacity(64 + 16);
    outer_data.extend_from_slice(&o_key_pad);
    outer_data.extend_from_slice(&inner_hash);
    md5_digest(&outer_data)
}

fn extract_bytes(val: &Value) -> Vec<u8> {
    if let Some(s) = val.as_str() {
        s.as_bytes().to_vec()
    } else if val.is_array() {
        unsafe {
            if let GcData::Array(ref arr) = (*val.as_gc_ptr()).data {
                let mut bytes = Vec::with_capacity(arr.len());
                for item in arr {
                    bytes.push(item.as_number() as u8);
                }
                bytes
            } else {
                Vec::new()
            }
        }
    } else {
        val.to_string().into_bytes()
    }
}

pub fn native_crypto_hash(args: Vec<Value>) -> Value {
    if args.is_empty() {
        return Value::null();
    }
    let (algo, data_val) = if args.len() >= 2 {
        let a = args[0].as_str().unwrap_or("sha256").to_lowercase();
        (a, &args[1])
    } else {
        ("sha256".to_string(), &args[0])
    };

    let bytes = extract_bytes(data_val);

    let hex_res = match algo.as_str() {
        "sha256" => {
            let d = digest::digest(&SHA256, &bytes);
            hex_encode(d.as_ref())
        }
        "sha512" => {
            let d = digest::digest(&SHA512, &bytes);
            hex_encode(d.as_ref())
        }
        "md5" => {
            let d = md5_digest(&bytes);
            hex_encode(&d)
        }
        _ => {
            let d = digest::digest(&SHA256, &bytes);
            hex_encode(d.as_ref())
        }
    };

    let ptr = gc_alloc_string(&hex_res);
    Value::string(ptr)
}

pub fn native_crypto_hmac(args: Vec<Value>) -> Value {
    if args.len() < 3 {
        return Value::null();
    }
    let algo = args[0].as_str().unwrap_or("sha256").to_lowercase();
    let key_bytes = extract_bytes(&args[1]);
    let data_bytes = extract_bytes(&args[2]);

    let hex_res = match algo.as_str() {
        "sha256" => {
            let key = hmac::Key::new(hmac::HMAC_SHA256, &key_bytes);
            let signature = hmac::sign(&key, &data_bytes);
            hex_encode(signature.as_ref())
        }
        "sha512" => {
            let key = hmac::Key::new(hmac::HMAC_SHA512, &key_bytes);
            let signature = hmac::sign(&key, &data_bytes);
            hex_encode(signature.as_ref())
        }
        "md5" => {
            let sig = hmac_md5(&key_bytes, &data_bytes);
            hex_encode(&sig)
        }
        _ => {
            let key = hmac::Key::new(hmac::HMAC_SHA256, &key_bytes);
            let signature = hmac::sign(&key, &data_bytes);
            hex_encode(signature.as_ref())
        }
    };

    let ptr = gc_alloc_string(&hex_res);
    Value::string(ptr)
}

pub fn native_crypto_timing_safe_equal(args: Vec<Value>) -> Value {
    if args.len() < 2 {
        return Value::boolean(false);
    }
    let a = extract_bytes(&args[0]);
    let b = extract_bytes(&args[1]);

    if a.len() != b.len() {
        return Value::boolean(false);
    }

    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    Value::boolean(diff == 0)
}

pub fn native_crypto_random_uuid(_args: Vec<Value>) -> Value {
    let rng = SystemRandom::new();
    let mut bytes = [0u8; 16];
    if rng.fill(&mut bytes).is_err() {
        // Fallback to timestamp + pseudo random if system random failed
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let now_bytes = now.to_le_bytes();
        bytes[..16].copy_from_slice(&now_bytes[..16]);
    }

    // Set version 4 (0100 in bits 4-7 of time_hi_and_version)
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    // Set variant 1 (10 in bits 6-7 of clock_seq_hi_and_reserved)
    bytes[8] = (bytes[8] & 0x3f) | 0x80;

    let uuid_str = format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        bytes[0], bytes[1], bytes[2], bytes[3],
        bytes[4], bytes[5],
        bytes[6], bytes[7],
        bytes[8], bytes[9],
        bytes[10], bytes[11], bytes[12], bytes[13], bytes[14], bytes[15]
    );

    let ptr = gc_alloc_string(&uuid_str);
    Value::string(ptr)
}

pub fn native_crypto_random_bytes(args: Vec<Value>) -> Value {
    let len = if !args.is_empty() && args[0].is_number() {
        args[0].as_number() as usize
    } else {
        32
    };

    let rng = SystemRandom::new();
    let mut buf = vec![0u8; len];
    let _ = rng.fill(&mut buf);

    let mut arr = get_pooled_vec(len);
    for b in buf {
        arr.push(Value::number(b as f64));
    }

    let ptr = gc_allocate(GcData::Array(arr));
    Value::array(ptr)
}

pub fn native_crypto_random_hex(args: Vec<Value>) -> Value {
    let len = if !args.is_empty() && args[0].is_number() {
        args[0].as_number() as usize
    } else {
        16
    };

    let rng = SystemRandom::new();
    let mut buf = vec![0u8; len];
    let _ = rng.fill(&mut buf);

    let hex_str = hex_encode(&buf);
    let ptr = gc_alloc_string(&hex_str);
    Value::string(ptr)
}

pub fn native_crypto_random_int(args: Vec<Value>) -> Value {
    if args.is_empty() {
        return Value::number(0.0);
    }
    let (min, max) = if args.len() >= 2 {
        (args[0].as_number() as i64, args[1].as_number() as i64)
    } else {
        (0i64, args[0].as_number() as i64)
    };

    if min >= max {
        return Value::number(min as f64);
    }

    let range = (max - min) as u64;
    let rng = SystemRandom::new();
    let mut bytes = [0u8; 8];
    let _ = rng.fill(&mut bytes);
    let r = u64::from_le_bytes(bytes);
    let val = min + ((r % range) as i64);

    Value::number(val as f64)
}

pub fn register_crypto_natives(vm: &mut VM) {
    vm.register_global("Eronom_nativeCryptoHash", Value::native_function(native_crypto_hash));
    vm.register_global("Eronom_nativeCryptoHmac", Value::native_function(native_crypto_hmac));
    vm.register_global("Eronom_nativeCryptoTimingSafeEqual", Value::native_function(native_crypto_timing_safe_equal));
    vm.register_global("Eronom_nativeCryptoRandomUUID", Value::native_function(native_crypto_random_uuid));
    vm.register_global("Eronom_nativeCryptoRandomBytes", Value::native_function(native_crypto_random_bytes));
    vm.register_global("Eronom_nativeCryptoRandomHex", Value::native_function(native_crypto_random_hex));
    vm.register_global("Eronom_nativeCryptoRandomInt", Value::native_function(native_crypto_random_int));
}
