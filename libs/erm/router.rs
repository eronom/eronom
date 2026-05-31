use std::collections::HashMap;

pub type H = HashMap<String, String>;

pub struct Ctx {
    pub params: HashMap<String, String>,
    pub status: u16,
}

impl Ctx {
    pub fn param(&self, name: &str) -> &str {
        self.params.get(name).map(|s| s.as_str()).unwrap_or("")
    }

    pub fn set_status(&mut self, status: u16) -> &mut Self {
        self.status = status;
        self
    }

    pub fn json<T: serde::Serialize>(&mut self, data: &T) -> anyhow::Result<()> {
        let _json = serde_json::to_string(data)?;
        // TODO(port): respond with json
        Ok(())
    }

    pub fn send_string(&mut self, text: &str) -> anyhow::Result<()> {
        // TODO(port): respond with string
        let _ = text;
        Ok(())
    }
}

pub type HandlerFunc = fn(c: &mut Ctx) -> anyhow::Result<()>;

#[derive(PartialEq, Eq)]
pub enum Method {
    GET,
    POST,
    PUT,
    DELETE,
}

pub struct RouteEntry {
    pub method: Method,
    pub path: String,
    pub handler: HandlerFunc,
}

pub struct App {
    pub routes: Vec<RouteEntry>,
    pub prefix: String,
}

impl App {
    pub fn new() -> Self {
        Self {
            routes: Vec::new(),
            prefix: String::new(),
        }
    }

    pub fn handle(&mut self, method: Method, path: &str, h: HandlerFunc) -> anyhow::Result<()> {
        let mut full_path = self.prefix.clone();
        if !path.starts_with('/') {
            full_path.push('/');
        }
        full_path.push_str(path);

        // Trim trailing slash if not root
        if full_path.len() > 1 && full_path.ends_with('/') {
            full_path.pop();
        }

        self.routes.push(RouteEntry {
            method,
            path: full_path,
            handler: h,
        });
        Ok(())
    }

    pub fn get(&mut self, path: &str, h: HandlerFunc) -> anyhow::Result<()> {
        self.handle(Method::GET, path, h)
    }
    pub fn post(&mut self, path: &str, h: HandlerFunc) -> anyhow::Result<()> {
        self.handle(Method::POST, path, h)
    }
    pub fn put(&mut self, path: &str, h: HandlerFunc) -> anyhow::Result<()> {
        self.handle(Method::PUT, path, h)
    }
    pub fn delete(&mut self, path: &str, h: HandlerFunc) -> anyhow::Result<()> {
        self.handle(Method::DELETE, path, h)
    }
}

pub fn match_path(pattern: &str, path: &str) -> Option<HashMap<String, String>> {
    if pattern == "/" && path == "/" {
        return Some(HashMap::new());
    }

    let pat_parts: Vec<&str> = pattern.split('/').filter(|s| !s.is_empty()).collect();
    let path_parts: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();

    if pat_parts.len() != path_parts.len() {
        return None;
    }

    let mut params = HashMap::new();
    for (pat_part, path_part) in pat_parts.iter().zip(path_parts.iter()) {
        if pat_part.starts_with(':') {
            params.insert(pat_part[1..].to_string(), path_part.to_string());
        } else if pat_part != path_part {
            return None;
        }
    }

    Some(params)
}
