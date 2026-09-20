use crate::frontend::lexer::lex;
use crate::frontend::parser::Parser;
use super::check_program;

fn parse_and_check(code: &str) -> Result<(), String> {
    let tokens = lex(code);
    let mut parser = Parser::new(tokens);
    let stmts = parser.parse().map_err(|e| format!("Parse error: {}", e))?;
    check_program(&stmts).map_err(|errs| {
        errs.into_iter()
            .map(|e| e.to_string())
            .collect::<Vec<_>>()
            .join("; ")
    })
}

#[test]
fn test_primitive_valid_and_invalid() {
    let valid_code = r#"
        let a: int = 42;
        let b: string = "hello";
        let c: bool = true;
        let d: float = 3.14;
    "#;
    assert!(parse_and_check(valid_code).is_ok());

    let invalid_code = r#"
        let x: string = 42;
    "#;
    assert!(parse_and_check(invalid_code).is_err());
}

#[test]
fn test_array_types() {
    let valid_code = r#"
        let nums: int[] = [1, 2, 3];
        let words: string[] = ["a", "b", "c"];
    "#;
    assert!(parse_and_check(valid_code).is_ok());

    let invalid_code = r#"
        let bad: int[] = ["hello", "world"];
    "#;
    assert!(parse_and_check(invalid_code).is_err());
}

#[test]
fn test_union_types() {
    let valid_code = r#"
        let id: string | int = "abc";
        id = 123;
    "#;
    assert!(parse_and_check(valid_code).is_ok());

    let invalid_code = r#"
        let id: string | int = true;
    "#;
    assert!(parse_and_check(invalid_code).is_err());
}

#[test]
fn test_nullable_types() {
    let valid_code = r#"
        let name: string? = null;
        name = "eronom";
    "#;
    assert!(parse_and_check(valid_code).is_ok());

    let invalid_code = r#"
        let name: string? = 100;
    "#;
    assert!(parse_and_check(invalid_code).is_err());
}

#[test]
fn test_type_alias() {
    let valid_code = r#"
        type Point = { x: int, y: int };
        let p: Point = { x: 10, y: 20 };

        type ID = string | int;
        let val: ID = 42;
    "#;
    if let Err(e) = parse_and_check(valid_code) {
        panic!("test_type_alias failed: {}", e);
    }
}

#[test]
fn test_enum() {
    let valid_code = r#"
        enum Direction { Up, Down, Left, Right };
        let dir: number = Direction.Up;
    "#;
    if let Err(e) = parse_and_check(valid_code) {
        panic!("test_enum failed: {}", e);
    }
}

#[test]
fn test_function_type_and_return() {
    let valid_code = r#"
        fn add(a: int, b: int): int {
            return a + b;
        }
        let res: int = add(5, 10);
    "#;
    assert!(parse_and_check(valid_code).is_ok());

    let invalid_code = r#"
        fn bad(a: int): string {
            return a;
        }
    "#;
    assert!(parse_and_check(invalid_code).is_err());
}

#[test]
fn test_type_narrowing() {
    let valid_typeof = r#"
        let val: string | int = "hello";
        if (typeof val == "string") {
            let s: string = val;
        }
    "#;
    assert!(parse_and_check(valid_typeof).is_ok());

    let valid_null_check = r#"
        let opt: string? = "hello";
        if (opt != null) {
            let s: string = opt;
        }
    "#;
    assert!(parse_and_check(valid_null_check).is_ok());
}

#[test]
fn test_type_assertion_as() {
    let valid_code = r#"
        let x: any = 42;
        let y: int = x as int;
    "#;
    if let Err(e) = parse_and_check(valid_code) {
        panic!("test_type_assertion_as failed: {}", e);
    }
}
