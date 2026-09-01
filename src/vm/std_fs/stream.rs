use std::cell::RefCell;
use std::collections::HashMap;
use std::fs::File;
use std::io::Read;
use std::path::Path;

use crate::vm::gc::{gc_alloc_string, gc_allocate, get_pooled_vec, GcData};
use crate::vm::value::Value;

struct ReadStreamState {
    file: File,
}

thread_local! {
    static NEXT_STREAM_ID: RefCell<u64> = const { RefCell::new(1) };
    static READ_STREAMS: RefCell<HashMap<u64, ReadStreamState>> = RefCell::new(HashMap::new());
}

pub fn native_fs_open_read_stream(args: Vec<Value>) -> Value {
    if args.is_empty() {
        return Value::null();
    }
    let path_str = match args[0].as_str() {
        Some(s) => s,
        None => return Value::null(),
    };

    let file = match File::open(Path::new(path_str)) {
        Ok(f) => f,
        Err(_) => return Value::null(),
    };

    let id = NEXT_STREAM_ID.with(|id_cell| {
        let cur = *id_cell.borrow();
        *id_cell.borrow_mut() = cur + 1;
        cur
    });

    READ_STREAMS.with(|map_cell| {
        map_cell.borrow_mut().insert(id, ReadStreamState { file });
    });

    Value::number(id as f64)
}

pub fn native_fs_read_stream_chunk(args: Vec<Value>) -> Value {
    if args.is_empty() {
        return Value::null();
    }
    let id = args[0].as_number() as u64;
    let chunk_size = if args.len() > 1 && args[1].is_number() {
        args[1].as_number() as usize
    } else {
        65536
    };

    READ_STREAMS.with(|map_cell| {
        let mut map = map_cell.borrow_mut();
        if let Some(state) = map.get_mut(&id) {
            let mut buf = vec![0u8; chunk_size];
            match state.file.read(&mut buf) {
                Ok(0) => {
                    map.remove(&id);
                    Value::null()
                }
                Ok(n) => {
                    buf.truncate(n);
                    let s = String::from_utf8_lossy(&buf);
                    let ptr = gc_alloc_string(&s);
                    Value::string(ptr)
                }
                Err(_) => {
                    map.remove(&id);
                    Value::null()
                }
            }
        } else {
            Value::null()
        }
    })
}

pub fn native_fs_read_stream_binary_chunk(args: Vec<Value>) -> Value {
    if args.is_empty() {
        return Value::null();
    }
    let id = args[0].as_number() as u64;
    let chunk_size = if args.len() > 1 && args[1].is_number() {
        args[1].as_number() as usize
    } else {
        65536
    };

    READ_STREAMS.with(|map_cell| {
        let mut map = map_cell.borrow_mut();
        if let Some(state) = map.get_mut(&id) {
            let mut buf = vec![0u8; chunk_size];
            match state.file.read(&mut buf) {
                Ok(0) => {
                    map.remove(&id);
                    Value::null()
                }
                Ok(n) => {
                    buf.truncate(n);
                    let mut arr = get_pooled_vec(n);
                    for b in buf {
                        arr.push(Value::number(b as f64));
                    }
                    let ptr = gc_allocate(GcData::Array(arr));
                    Value::array(ptr)
                }
                Err(_) => {
                    map.remove(&id);
                    Value::null()
                }
            }
        } else {
            Value::null()
        }
    })
}

pub fn native_fs_close_read_stream(args: Vec<Value>) -> Value {
    if args.is_empty() {
        return Value::boolean(false);
    }
    let id = args[0].as_number() as u64;
    let removed = READ_STREAMS.with(|map_cell| map_cell.borrow_mut().remove(&id).is_some());
    Value::boolean(removed)
}
