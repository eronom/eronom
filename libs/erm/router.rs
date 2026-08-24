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

#[derive(PartialEq, Eq, Clone, Copy)]
pub enum Method {
    GET,
    POST,
    PUT,
    DELETE,
    PATCH,
    HEAD,
    OPTIONS,
    ALL,
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
    pub fn patch(&mut self, path: &str, h: HandlerFunc) -> anyhow::Result<()> {
        self.handle(Method::PATCH, path, h)
    }
    pub fn head(&mut self, path: &str, h: HandlerFunc) -> anyhow::Result<()> {
        self.handle(Method::HEAD, path, h)
    }
    pub fn options(&mut self, path: &str, h: HandlerFunc) -> anyhow::Result<()> {
        self.handle(Method::OPTIONS, path, h)
    }
    pub fn all(&mut self, path: &str, h: HandlerFunc) -> anyhow::Result<()> {
        self.handle(Method::ALL, path, h)
    }
}

pub fn match_path(pattern: &str, path: &str) -> Option<HashMap<String, String>> {
    if pattern == "/" && path == "/" {
        return Some(HashMap::new());
    }
    if pattern == "*" || pattern == "/*" {
        let mut params = HashMap::new();
        let tail = if path.starts_with('/') { &path[1..] } else { path };
        params.insert("*".to_string(), tail.to_string());
        return Some(params);
    }

    let pat_parts: Vec<&str> = pattern.split('/').filter(|s| !s.is_empty()).collect();
    let path_parts: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();

    let mut params = HashMap::new();
    for (i, pat_part) in pat_parts.iter().enumerate() {
        if *pat_part == "*" || pat_part.starts_with('*') {
            if i == pat_parts.len() - 1 {
                let tail = if i < path_parts.len() {
                    path_parts[i..].join("/")
                } else {
                    String::new()
                };
                let key = if *pat_part == "*" { "*" } else { &pat_part[1..] };
                params.insert(key.to_string(), tail);
                return Some(params);
            }
        }

        if i >= path_parts.len() {
            return None;
        }

        let path_part = path_parts[i];
        if pat_part.starts_with(':') {
            params.insert(pat_part[1..].to_string(), path_part.to_string());
        } else if pat_part != &path_part {
            return None;
        }
    }

    if pat_parts.len() == path_parts.len() {
        Some(params)
    } else {
        None
    }
}
