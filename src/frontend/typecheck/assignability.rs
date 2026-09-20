use crate::frontend::ast::{PrimitiveType, TypeNode};
use super::env::TypeEnv;

pub fn is_assignable(expected: &TypeNode, actual: &TypeNode, env: &TypeEnv) -> bool {
    let exp_resolved = env.resolve_type(expected);
    let act_resolved = env.resolve_type(actual);

    // 1. Any type: opt-out escape hatch
    if matches!(exp_resolved, TypeNode::Primitive(PrimitiveType::Any))
        || matches!(act_resolved, TypeNode::Primitive(PrimitiveType::Any))
    {
        return true;
    }

    // 2. Exact match
    if exp_resolved == act_resolved {
        return true;
    }

    // 3. Unknown can receive anything
    if matches!(exp_resolved, TypeNode::Primitive(PrimitiveType::Unknown)) {
        return true;
    }

    // 4. Never can be assigned to anything (bottom type)
    if matches!(act_resolved, TypeNode::Primitive(PrimitiveType::Never)) {
        return true;
    }

    // 5. Nullable unpacking: T? is T | null
    if let TypeNode::Nullable(inner) = &exp_resolved {
        let union_exp = TypeNode::Union(vec![*inner.clone(), TypeNode::Primitive(PrimitiveType::Null)]);
        return is_assignable(&union_exp, &act_resolved, env);
    }
    if let TypeNode::Nullable(inner) = &act_resolved {
        let union_act = TypeNode::Union(vec![*inner.clone(), TypeNode::Primitive(PrimitiveType::Null)]);
        return is_assignable(&exp_resolved, &union_act, env);
    }

    // 6. Union handling
    if let TypeNode::Union(actual_members) = &act_resolved {
        // For actual to be assignable to expected, every variant of actual must be assignable to expected
        return actual_members.iter().all(|m| is_assignable(&exp_resolved, m, env));
    }

    if let TypeNode::Union(expected_members) = &exp_resolved {
        // If expected is a union, actual is assignable if it matches ANY variant
        return expected_members.iter().any(|m| is_assignable(m, &act_resolved, env));
    }

    // 7. Intersection handling
    if let TypeNode::Intersection(expected_members) = &exp_resolved {
        // Actual must satisfy ALL members of the expected intersection
        return expected_members.iter().all(|m| is_assignable(m, &act_resolved, env));
    }

    if let TypeNode::Intersection(actual_members) = &act_resolved {
        // If actual has all types, it satisfies expected if ANY component satisfies it
        return actual_members.iter().any(|m| is_assignable(&exp_resolved, m, env));
    }

    // 8. Literal types assignable to their base primitive
    if let TypeNode::Literal(lit) = &act_resolved {
        if lit.starts_with('"') && matches!(exp_resolved, TypeNode::Primitive(PrimitiveType::String)) {
            return true;
        }
        if (lit == "true" || lit == "false") && matches!(exp_resolved, TypeNode::Primitive(PrimitiveType::Bool)) {
            return true;
        }
        if lit.parse::<f64>().is_ok()
            && matches!(
                exp_resolved,
                TypeNode::Primitive(PrimitiveType::Int)
                    | TypeNode::Primitive(PrimitiveType::Float)
                    | TypeNode::Primitive(PrimitiveType::Number)
            )
        {
            return true;
        }
    }

    // 9. Numeric subtyping: int and float are assignable to number
    if matches!(exp_resolved, TypeNode::Primitive(PrimitiveType::Number))
        && matches!(
            act_resolved,
            TypeNode::Primitive(PrimitiveType::Int) | TypeNode::Primitive(PrimitiveType::Float)
        )
    {
        return true;
    }
    // int <-> float interoperability in scripts
    if matches!(exp_resolved, TypeNode::Primitive(PrimitiveType::Float))
        && matches!(act_resolved, TypeNode::Primitive(PrimitiveType::Int))
    {
        return true;
    }
    if matches!(exp_resolved, TypeNode::Primitive(PrimitiveType::Int))
        && matches!(act_resolved, TypeNode::Primitive(PrimitiveType::Float))
    {
        return true;
    }

    // 10. Arrays
    if let (TypeNode::Array(exp_elem), TypeNode::Array(act_elem)) = (&exp_resolved, &act_resolved) {
        return is_assignable(exp_elem, act_elem, env);
    }

    // 11. Tuples
    if let (TypeNode::Tuple(exp_elems), TypeNode::Tuple(act_elems)) = (&exp_resolved, &act_resolved) {
        if exp_elems.len() != act_elems.len() {
            return false;
        }
        return exp_elems.iter().zip(act_elems.iter()).all(|(e, a)| is_assignable(e, a, env));
    }

    // 12. Function subtyping
    if let (
        TypeNode::Function {
            params: exp_params,
            return_type: exp_ret,
        },
        TypeNode::Function {
            params: act_params,
            return_type: act_ret,
        },
    ) = (&exp_resolved, &act_resolved)
    {
        // Parameter count must match or act can accept fewer (standard JS/TS rule)
        if act_params.len() > exp_params.len() {
            return false;
        }
        // Contravariant parameters: exp_param must be assignable to act_param
        for (ep, ap) in exp_params.iter().zip(act_params.iter()) {
            if !is_assignable(ap, ep, env) {
                return false;
            }
        }
        // Covariant return type: act_ret must be assignable to exp_ret
        return is_assignable(exp_ret, act_ret, env);
    }

    // 13. Structural object subtyping
    if let TypeNode::Object(exp_props) = &exp_resolved {
        match &act_resolved {
            TypeNode::Object(act_props) => {
                let act_map: std::collections::HashMap<&str, &TypeNode> =
                    act_props.iter().map(|p| (p.name.as_str(), &p.ty)).collect();

                for exp_p in exp_props {
                    if let Some(act_ty) = act_map.get(exp_p.name.as_str()) {
                        if !is_assignable(&exp_p.ty, act_ty, env) {
                            return false;
                        }
                    } else if !exp_p.optional {
                        return false;
                    }
                }
                return true;
            }
            TypeNode::Named(struct_name, _) => {
                if let Some(fields) = env.get_struct(struct_name) {
                    let field_map: std::collections::HashMap<&str, &TypeNode> =
                        fields.iter().map(|(n, t)| (n.as_str(), t)).collect();

                    for exp_p in exp_props {
                        if let Some(act_ty) = field_map.get(exp_p.name.as_str()) {
                            if !is_assignable(&exp_p.ty, act_ty, env) {
                                return false;
                            }
                        } else if !exp_p.optional {
                            return false;
                        }
                    }
                    return true;
                }
            }
            _ => {}
        }
    }

    // 14. Interface satisfaction
    if let TypeNode::Named(ref name, _) = exp_resolved {
        if let Some(iface_fields) = env.get_interface(name) {
            match &act_resolved {
                TypeNode::Object(act_props) => {
                    let act_map: std::collections::HashMap<&str, &TypeNode> =
                        act_props.iter().map(|p| (p.name.as_str(), &p.ty)).collect();

                    for (req_name, req_ty) in iface_fields {
                        if let Some(act_ty) = act_map.get(req_name.as_str()) {
                            if !is_assignable(req_ty, act_ty, env) {
                                return false;
                            }
                        } else {
                            return false;
                        }
                    }
                    return true;
                }
                TypeNode::Named(act_name, _) => {
                    if let Some(struct_fields) = env.get_struct(act_name) {
                        let field_map: std::collections::HashMap<&str, &TypeNode> =
                            struct_fields.iter().map(|(n, t)| (n.as_str(), t)).collect();

                        for (req_name, req_ty) in iface_fields {
                            if let Some(act_ty) = field_map.get(req_name.as_str()) {
                                if !is_assignable(req_ty, act_ty, env) {
                                    return false;
                                }
                            } else {
                                return false;
                            }
                        }
                        return true;
                    }
                }
                _ => {}
            }
        }
    }

    false
}
