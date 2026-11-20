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
            let mut out = [0u8; 32];
            unsafe {
                eronom_net::ffi::SHA256(bytes.as_ptr(), bytes.len(), out.as_mut_ptr());
            }
            hex_encode(&out)
        }
        "sha512" => {
            let mut out = [0u8; 64];
            unsafe {
                eronom_net::ffi::SHA512(bytes.as_ptr(), bytes.len(), out.as_mut_ptr());
            }
            hex_encode(&out)
        }
        "md5" => {
            let mut out = [0u8; 16];
            unsafe {
                eronom_net::ffi::MD5(bytes.as_ptr(), bytes.len(), out.as_mut_ptr());
            }
            hex_encode(&out)
        }
        _ => {
            let mut out = [0u8; 32];
            unsafe {
                eronom_net::ffi::SHA256(bytes.as_ptr(), bytes.len(), out.as_mut_ptr());
            }
            hex_encode(&out)
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

    let evp_md = match algo.as_str() {
        "sha256" => unsafe { eronom_net::ffi::EVP_sha256() },
        "sha512" => unsafe { eronom_net::ffi::EVP_sha512() },
        "md5" => unsafe { eronom_net::ffi::EVP_md5() },
        _ => unsafe { eronom_net::ffi::EVP_sha256() },
    };

    let mut out = [0u8; 64];
    let mut out_len: u32 = 0;
    let res_ptr = unsafe {
        eronom_net::ffi::HMAC(
            evp_md,
            key_bytes.as_ptr() as *const std::ffi::c_void,
            key_bytes.len(),
            data_bytes.as_ptr(),
            data_bytes.len(),
            out.as_mut_ptr(),
            &mut out_len,
        )
    };

    let hex_res = if !res_ptr.is_null() && (out_len as usize) <= out.len() {
        hex_encode(&out[..out_len as usize])
    } else {
        String::new()
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

    let rc = unsafe {
        eronom_net::ffi::CRYPTO_memcmp(
            a.as_ptr() as *const std::ffi::c_void,
            b.as_ptr() as *const std::ffi::c_void,
            a.len(),
        )
    };
    Value::boolean(rc == 0)
}

pub fn native_crypto_random_uuid(_args: Vec<Value>) -> Value {
    let mut bytes = [0u8; 16];
    let rc = unsafe { eronom_net::ffi::RAND_bytes(bytes.as_mut_ptr(), bytes.len()) };
    if rc != 1 {
        // Fallback to timestamp if RAND_bytes failed
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

    let mut buf = vec![0u8; len];
    if len > 0 {
        unsafe {
            eronom_net::ffi::RAND_bytes(buf.as_mut_ptr(), len);
        }
    }

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

    let mut buf = vec![0u8; len];
    if len > 0 {
        unsafe {
            eronom_net::ffi::RAND_bytes(buf.as_mut_ptr(), len);
        }
    }

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
    let mut bytes = [0u8; 8];
    unsafe {
        eronom_net::ffi::RAND_bytes(bytes.as_mut_ptr(), 8);
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_crypto_sha256_and_md5_hash() {
        let str_ptr = gc_alloc_string("hello");
        let val = Value::string(str_ptr);

        let algo_ptr = gc_alloc_string("sha256");
        let algo_val = Value::string(algo_ptr);
        let res = native_crypto_hash(vec![algo_val, val]);
        assert_eq!(
            res.as_str().unwrap(),
            "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
        );

        let md5_ptr = gc_alloc_string("md5");
        let md5_algo = Value::string(md5_ptr);
        let res_md5 = native_crypto_hash(vec![md5_algo, val]);
        assert_eq!(res_md5.as_str().unwrap(), "5d41402abc4b2a76b9719d911017c592");
    }

    #[test]
    fn test_crypto_hmac_sha256() {
        let algo = Value::string(gc_alloc_string("sha256"));
        let key = Value::string(gc_alloc_string("key"));
        let data = Value::string(gc_alloc_string("The quick brown fox jumps over the lazy dog"));
        let hmac_res = native_crypto_hmac(vec![algo, key, data]);
        assert_eq!(
            hmac_res.as_str().unwrap(),
            "f7bc83f430538424b13298e6aa6fb143ef4d59a14946175997479dbc2d1a3cd8"
        );
    }

    #[test]
    fn test_crypto_random_uuid_format() {
        let uuid_val = native_crypto_random_uuid(vec![]);
        let uuid_str = uuid_val.as_str().unwrap();
        assert_eq!(uuid_str.len(), 36);
        assert_eq!(&uuid_str[14..15], "4"); // UUID v4 version nibble
        let c = uuid_str.chars().nth(19).unwrap();
        assert!(c == '8' || c == '9' || c == 'a' || c == 'b'); // variant 1
    }
}
