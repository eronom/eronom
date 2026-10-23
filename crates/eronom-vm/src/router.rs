use std::collections::HashMap;
use crate::vm::value::Value;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SegmentType {
    Root,
    Static(String),
    Param(String),      // e.g. "id" for ":id"
    Wildcard(String),   // e.g. "*" or "path" for "*path"
}

#[derive(Debug, Clone)]
pub struct RouteNode {
    pub segment: SegmentType,
    pub children: Vec<RouteNode>,
    pub handlers: HashMap<String, Value>, // method (e.g., "GET", "POST", "ALL") -> handler
}

impl RouteNode {
    pub fn new(segment: SegmentType) -> Self {
        Self {
            segment,
            children: Vec::new(),
            handlers: HashMap::new(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct RadixRouter {
    pub root: RouteNode,
}

impl RadixRouter {
    pub fn new() -> Self {
        Self {
            root: RouteNode::new(SegmentType::Root),
        }
    }

    pub fn clear(&mut self) {
        self.root = RouteNode::new(SegmentType::Root);
    }

    /// Split a path into normalized non-empty segments
    fn split_path(path: &str) -> Vec<&str> {
        let trimmed = path.trim();
        // Remove query string if present
        let path_only = match trimmed.find('?') {
            Some(idx) => &trimmed[..idx],
            None => trimmed,
        };
        path_only
            .split('/')
            .filter(|s| !s.is_empty())
            .collect()
    }

    /// Insert a route with a specific HTTP method and path pattern
    pub fn insert(&mut self, method: &str, path: &str, handler: Value) {
        let method_upper = method.trim().to_ascii_uppercase();
        let segments = Self::split_path(path);
        
        let mut current = &mut self.root;

        for seg in segments {
            let seg_type = if seg.starts_with('*') {
                let name = if seg == "*" { "*" } else { &seg[1..] };
                SegmentType::Wildcard(name.to_string())
            } else if seg.starts_with(':') {
                let name = &seg[1..];
                SegmentType::Param(name.to_string())
            } else {
                SegmentType::Static(seg.to_string())
            };

            // Find or insert child
            let child_idx = if let Some(idx) = current.children.iter().position(|c| c.segment == seg_type) {
                idx
            } else {
                let new_node = RouteNode::new(seg_type.clone());
                current.children.push(new_node);
                // Sort children so Static comes before Param, and Param comes before Wildcard
                current.children.sort_by(|a, b| {
                    match (&a.segment, &b.segment) {
                        (SegmentType::Static(_), SegmentType::Static(_)) => std::cmp::Ordering::Equal,
                        (SegmentType::Static(_), _) => std::cmp::Ordering::Less,
                        (_, SegmentType::Static(_)) => std::cmp::Ordering::Greater,
                        (SegmentType::Param(_), SegmentType::Param(_)) => std::cmp::Ordering::Equal,
                        (SegmentType::Param(_), SegmentType::Wildcard(_)) => std::cmp::Ordering::Less,
                        (SegmentType::Wildcard(_), SegmentType::Param(_)) => std::cmp::Ordering::Greater,
                        (SegmentType::Wildcard(_), SegmentType::Wildcard(_)) => std::cmp::Ordering::Equal,
                        _ => std::cmp::Ordering::Equal,
                    }
                });
                current.children.iter().position(|c| c.segment == seg_type).unwrap()
            };

            current = &mut current.children[child_idx];
        }

        current.handlers.insert(method_upper, handler);
    }

    /// Match an incoming request (method + path), extracting route parameters
    pub fn find(&self, method: &str, path: &str) -> Option<(Value, HashMap<String, String>)> {
        let method_upper = method.trim().to_ascii_uppercase();
        let segments = Self::split_path(path);
        let mut params = HashMap::new();

        if self.match_node(&self.root, &segments, 0, &method_upper, &mut params, &mut None) {
            if let Some(handler) = self.get_handler(&self.root, &segments, 0, &method_upper, &mut params) {
                return Some((handler, params));
            }
        }

        None
    }

    fn get_handler(
        &self,
        node: &RouteNode,
        segments: &[&str],
        seg_idx: usize,
        method: &str,
        params: &mut HashMap<String, String>,
    ) -> Option<Value> {
        // Base case: we've consumed all segments
        if seg_idx == segments.len() {
            // Check for exact method or fallback to "ALL" / "*"
            if let Some(&handler) = node.handlers.get(method) {
                return Some(handler);
            }
            if let Some(&handler) = node.handlers.get("ALL") {
                return Some(handler);
            }
            if let Some(&handler) = node.handlers.get("*") {
                return Some(handler);
            }
            if let Some(&handler) = node.handlers.get("ANY") {
                return Some(handler);
            }
            // Check if there is a wildcard child that handles this
            for child in &node.children {
                if let SegmentType::Wildcard(name) = &child.segment {
                    if let Some(&handler) = child.handlers.get(method).or_else(|| child.handlers.get("ALL")).or_else(|| child.handlers.get("*")) {
                        params.insert(name.clone(), String::new());
                        return Some(handler);
                    }
                }
            }
            return None;
        }

        let seg = segments[seg_idx];

        // 1. Try static children first
        for child in &node.children {
            if let SegmentType::Static(s) = &child.segment {
                if s == seg {
                    if let Some(h) = self.get_handler(child, segments, seg_idx + 1, method, params) {
                        return Some(h);
                    }
                }
            }
        }

        // 2. Try param children second
        for child in &node.children {
            if let SegmentType::Param(param_name) = &child.segment {
                if let Some(h) = self.get_handler(child, segments, seg_idx + 1, method, params) {
                    params.insert(param_name.clone(), seg.to_string());
                    return Some(h);
                }
            }
        }

        // 3. Try wildcard children third
        for child in &node.children {
            if let SegmentType::Wildcard(wildcard_name) = &child.segment {
                if let Some(&handler) = child.handlers.get(method).or_else(|| child.handlers.get("ALL")).or_else(|| child.handlers.get("*")) {
                    let remaining = segments[seg_idx..].join("/");
                    params.insert(wildcard_name.clone(), remaining);
                    return Some(handler);
                }
            }
        }

        None
    }

    fn match_node(
        &self,
        node: &RouteNode,
        segments: &[&str],
        seg_idx: usize,
        method: &str,
        params: &mut HashMap<String, String>,
        matched_handler: &mut Option<Value>,
    ) -> bool {
        if seg_idx == segments.len() {
            if let Some(&h) = node.handlers.get(method).or_else(|| node.handlers.get("ALL")).or_else(|| node.handlers.get("*")).or_else(|| node.handlers.get("ANY")) {
                *matched_handler = Some(h);
                return true;
            }
            for child in &node.children {
                if let SegmentType::Wildcard(name) = &child.segment {
                    if let Some(&h) = child.handlers.get(method).or_else(|| child.handlers.get("ALL")).or_else(|| child.handlers.get("*")) {
                        params.insert(name.clone(), String::new());
                        *matched_handler = Some(h);
                        return true;
                    }
                }
            }
            return false;
        }

        let seg = segments[seg_idx];

        // 1. Static
        for child in &node.children {
            if let SegmentType::Static(s) = &child.segment {
                if s == seg {
                    if self.match_node(child, segments, seg_idx + 1, method, params, matched_handler) {
                        return true;
                    }
                }
            }
        }

        // 2. Param
        for child in &node.children {
            if let SegmentType::Param(param_name) = &child.segment {
                let mut sub_params = HashMap::new();
                if self.match_node(child, segments, seg_idx + 1, method, &mut sub_params, matched_handler) {
                    params.extend(sub_params);
                    params.insert(param_name.clone(), seg.to_string());
                    return true;
                }
            }
        }

        // 3. Wildcard
        for child in &node.children {
            if let SegmentType::Wildcard(wildcard_name) = &child.segment {
                if let Some(&h) = child.handlers.get(method).or_else(|| child.handlers.get("ALL")).or_else(|| child.handlers.get("*")) {
                    let remaining = segments[seg_idx..].join("/");
                    params.insert(wildcard_name.clone(), remaining);
                    *matched_handler = Some(h);
                    return true;
                }
            }
        }

        false
    }

    /// Retrieve all registered handler values for GC marking
    pub fn collect_all_handlers(&self, out: &mut Vec<Value>) {
        Self::collect_handlers_recursive(&self.root, out);
    }

    fn collect_handlers_recursive(node: &RouteNode, out: &mut Vec<Value>) {
        for &h in node.handlers.values() {
            out.push(h);
        }
        for child in &node.children {
            Self::collect_handlers_recursive(child, out);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_radix_static_routes() {
        let mut router = RadixRouter::new();
        let h_root = Value::number(1.0);
        let h_users = Value::number(2.0);
        let h_user_new = Value::number(3.0);

        router.insert("GET", "/", h_root);
        router.insert("GET", "/users", h_users);
        router.insert("GET", "/users/new", h_user_new);

        assert_eq!(router.find("GET", "/").map(|(h, _)| h), Some(h_root));
        assert_eq!(router.find("GET", "/users").map(|(h, _)| h), Some(h_users));
        assert_eq!(router.find("GET", "/users/new").map(|(h, _)| h), Some(h_user_new));
        assert_eq!(router.find("GET", "/notfound"), None);
    }

    #[test]
    fn test_radix_parameter_routes() {
        let mut router = RadixRouter::new();
        let h_user_id = Value::number(10.0);
        let h_post_comments = Value::number(20.0);
        let h_static_profile = Value::number(30.0);

        router.insert("GET", "/users/:id", h_user_id);
        router.insert("GET", "/users/profile", h_static_profile);
        router.insert("GET", "/posts/:slug/comments/:commentId", h_post_comments);

        // Exact static route has precedence over :id
        let (h1, p1) = router.find("GET", "/users/profile").unwrap();
        assert_eq!(h1, h_static_profile);
        assert!(p1.is_empty());

        // Parameterized match
        let (h2, p2) = router.find("GET", "/users/42").unwrap();
        assert_eq!(h2, h_user_id);
        assert_eq!(p2.get("id").unwrap(), "42");

        // Multiple parameters
        let (h3, p3) = router.find("GET", "/posts/hello-world/comments/999").unwrap();
        assert_eq!(h3, h_post_comments);
        assert_eq!(p3.get("slug").unwrap(), "hello-world");
        assert_eq!(p3.get("commentId").unwrap(), "999");
    }

    #[test]
    fn test_radix_wildcards() {
        let mut router = RadixRouter::new();
        let h_files = Value::number(100.0);
        let h_all = Value::number(200.0);

        router.insert("GET", "/files/*filepath", h_files);
        router.insert("ALL", "/catchall/*", h_all);

        let (h1, p1) = router.find("GET", "/files/css/style.css").unwrap();
        assert_eq!(h1, h_files);
        assert_eq!(p1.get("filepath").unwrap(), "css/style.css");

        let (h2, p2) = router.find("POST", "/catchall/any/path/here").unwrap();
        assert_eq!(h2, h_all);
        assert_eq!(p2.get("*").unwrap(), "any/path/here");
    }
}
