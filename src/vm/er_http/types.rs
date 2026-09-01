use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::ffi::c_void;
use std::time::SystemTime;
use crate::vm::value::Value;
use crate::vm::execute::VM;
use crate::vm::router::RadixRouter;

#[derive(Clone)]
pub struct Route {
    pub method: String,
    pub path: String,
    pub callback: Value,
}

#[derive(Clone)]
pub struct WsRoute {
    pub path: String,
    pub open: Option<Value>,
    pub message: Option<Value>,
    pub close: Option<Value>,
}

#[derive(Debug, Clone)]
pub struct ResponseState {
    pub status: Option<u16>,
    pub headers: Vec<(String, String)>,
    pub cookies: Vec<String>,
    pub finished: bool,
}

impl Default for ResponseState {
    fn default() -> Self {
        Self::new()
    }
}

impl ResponseState {
    pub fn new() -> Self {
        Self {
            status: None,
            headers: Vec::new(),
            cookies: Vec::new(),
            finished: false,
        }
    }

    pub fn reset(&mut self) {
        self.status = None;
        self.headers.clear();
        self.cookies.clear();
        self.finished = false;
    }
}

thread_local! {
    pub static ROUTER: RefCell<RadixRouter> = RefCell::new(RadixRouter::new());
    pub static ROUTES: RefCell<Vec<Route>> = const { RefCell::new(Vec::new()) };
    pub static WS_ROUTES: RefCell<Vec<WsRoute>> = const { RefCell::new(Vec::new()) };
    pub static STATIC_MOUNTS: RefCell<Vec<(String, String)>> = const { RefCell::new(Vec::new()) };
    pub static MIDDLEWARES: RefCell<Vec<Value>> = const { RefCell::new(Vec::new()) };
    pub static ACTIVE_VM: Cell<*mut VM> = const { Cell::new(std::ptr::null_mut()) };
    pub static ACTIVE_HTTP_RESPONSE: Cell<*mut c_void> = const { Cell::new(std::ptr::null_mut()) };
    pub static ACTIVE_WEBSOCKET: Cell<*mut c_void> = const { Cell::new(std::ptr::null_mut()) };
    pub static ACTIVE_CONNECTIONS: RefCell<HashMap<*mut c_void, Value>> = RefCell::new(HashMap::new());
    pub static ROUTE_PREFIX: RefCell<Option<String>> = const { RefCell::new(None) };
    pub(crate) static TARGET_SCRIPT_PATH: RefCell<Option<String>> = const { RefCell::new(None) };
    pub(crate) static LAST_MTIME: Cell<Option<SystemTime>> = const { Cell::new(None) };
    pub(crate) static LAST_CHECK_TIME: Cell<Option<SystemTime>> = const { Cell::new(None) };
    pub static LISTEN_PORT: Cell<Option<i32>> = const { Cell::new(None) };
    pub static LISTEN_CALLBACK: RefCell<Option<Value>> = const { RefCell::new(None) };
    pub static SERVER_RUNNING: Cell<bool> = const { Cell::new(false) };

    pub static ACTIVE_REQUEST_HEADERS: RefCell<HashMap<String, String>> = RefCell::new(HashMap::new());
    pub static ACTIVE_REQUEST_COOKIES: RefCell<HashMap<String, String>> = RefCell::new(HashMap::new());
    pub static ACTIVE_REQUEST_QUERY: RefCell<HashMap<String, String>> = RefCell::new(HashMap::new());
    pub static ACTIVE_REQUEST_PATH: RefCell<String> = RefCell::new(String::new());
    pub static ACTIVE_RESPONSE_STATE: RefCell<ResponseState> = RefCell::new(ResponseState::new());
}
