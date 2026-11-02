use std::collections::HashMap;
use crate::frontend::ast::{PrimitiveType, TypeNode};

#[derive(Debug, Clone)]
pub struct TypeEnv {
    scopes: Vec<HashMap<String, TypeNode>>,
    type_aliases: HashMap<String, (Vec<String>, TypeNode)>,
    structs: HashMap<String, Vec<(String, TypeNode)>>,
    interfaces: HashMap<String, Vec<(String, TypeNode)>>,
    pub current_return_type: Option<TypeNode>,
}

impl Default for TypeEnv {
    fn default() -> Self {
        Self::new()
    }
}

impl TypeEnv {
    pub fn new() -> Self {
        let mut env = Self {
            scopes: vec![HashMap::new()],
            type_aliases: HashMap::new(),
            structs: HashMap::new(),
            interfaces: HashMap::new(),
            current_return_type: None,
        };

        // Standard global builtins
        env.define_var("print".to_string(), TypeNode::Function {
            params: vec![TypeNode::Primitive(PrimitiveType::Any)],
            return_type: Box::new(TypeNode::Primitive(PrimitiveType::Void)),
        });

        env
    }

    pub fn push_scope(&mut self) {
        self.scopes.push(HashMap::new());
    }

    pub fn pop_scope(&mut self) {
        if self.scopes.len() > 1 {
            self.scopes.pop();
        }
    }

    pub fn define_var(&mut self, name: String, ty: TypeNode) {
        if let Some(scope) = self.scopes.last_mut() {
            scope.insert(name, ty);
        }
    }

    pub fn set_var(&mut self, name: &str, ty: TypeNode) -> bool {
        for scope in self.scopes.iter_mut().rev() {
            if scope.contains_key(name) {
                scope.insert(name.to_string(), ty);
                return true;
            }
        }
        false
    }

    pub fn get_var(&self, name: &str) -> Option<&TypeNode> {
        for scope in self.scopes.iter().rev() {
            if let Some(ty) = scope.get(name) {
                return Some(ty);
            }
        }
        None
    }

    pub fn define_type_alias(&mut self, name: String, params: Vec<String>, ty: TypeNode) {
        self.type_aliases.insert(name, (params, ty));
    }

    pub fn define_struct(&mut self, name: String, fields: Vec<(String, TypeNode)>) {
        self.structs.insert(name, fields);
    }

    pub fn get_struct(&self, name: &str) -> Option<&Vec<(String, TypeNode)>> {
        self.structs.get(name)
    }

    pub fn define_interface(&mut self, name: String, fields: Vec<(String, TypeNode)>) {
        self.interfaces.insert(name, fields);
    }

    pub fn get_interface(&self, name: &str) -> Option<&Vec<(String, TypeNode)>> {
        self.interfaces.get(name)
    }

    pub fn resolve_type(&self, ty: &TypeNode) -> TypeNode {
        match ty {
            TypeNode::Named(name, args) => {
                if let Some((params, target)) = self.type_aliases.get(name) {
                    let mut substituted = target.clone();
                    if !params.is_empty() && params.len() == args.len() {
                        let subst_map: HashMap<&str, &TypeNode> =
                            params.iter().map(|p| p.as_str()).zip(args.iter()).collect();
                        substituted = substitute_type(&substituted, &subst_map);
                    }
                    return self.resolve_type(&substituted);
                }
                ty.clone()
            }
            TypeNode::Nullable(inner) => {
                let resolved_inner = self.resolve_type(inner);
                TypeNode::Union(vec![resolved_inner, TypeNode::Primitive(PrimitiveType::Null)])
            }
            TypeNode::Array(elem) => TypeNode::Array(Box::new(self.resolve_type(elem))),
            TypeNode::Union(members) => {
                TypeNode::Union(members.iter().map(|m| self.resolve_type(m)).collect())
            }
            TypeNode::Intersection(members) => {
                TypeNode::Intersection(members.iter().map(|m| self.resolve_type(m)).collect())
            }
            _ => ty.clone(),
        }
    }
}

fn substitute_type(ty: &TypeNode, subst: &HashMap<&str, &TypeNode>) -> TypeNode {
    match ty {
        TypeNode::Named(name, args) => {
            if let Some(&replacement) = subst.get(name.as_str()) {
                return replacement.clone();
            }
            TypeNode::Named(
                name.clone(),
                args.iter().map(|a| substitute_type(a, subst)).collect(),
            )
        }
        TypeNode::Array(elem) => TypeNode::Array(Box::new(substitute_type(elem, subst))),
        TypeNode::Tuple(elems) => {
            TypeNode::Tuple(elems.iter().map(|e| substitute_type(e, subst)).collect())
        }
        TypeNode::Union(members) => {
            TypeNode::Union(members.iter().map(|m| substitute_type(m, subst)).collect())
        }
        TypeNode::Intersection(members) => {
            TypeNode::Intersection(members.iter().map(|m| substitute_type(m, subst)).collect())
        }
        TypeNode::Nullable(inner) => TypeNode::Nullable(Box::new(substitute_type(inner, subst))),
        _ => ty.clone(),
    }
}
