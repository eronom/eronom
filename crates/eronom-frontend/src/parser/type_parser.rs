use super::Parser;
use crate::frontend::ast::{PrimitiveType, PropertySignature, TypeNode};
use crate::frontend::token::TokenType;

impl Parser {
    pub(crate) fn parse_type(&mut self) -> Result<TypeNode, String> {
        self.parse_union_type()
    }

    fn parse_union_type(&mut self) -> Result<TypeNode, String> {
        let mut members = vec![self.parse_intersection_type()?];
        while self.match_token(&[TokenType::Pipe]) {
            members.push(self.parse_intersection_type()?);
        }
        if members.len() == 1 {
            Ok(members.pop().unwrap())
        } else {
            Ok(TypeNode::Union(members))
        }
    }

    fn parse_intersection_type(&mut self) -> Result<TypeNode, String> {
        let mut members = vec![self.parse_postfix_type()?];
        while self.match_token(&[TokenType::Ampersand]) {
            members.push(self.parse_postfix_type()?);
        }
        if members.len() == 1 {
            Ok(members.pop().unwrap())
        } else {
            Ok(TypeNode::Intersection(members))
        }
    }

    fn parse_postfix_type(&mut self) -> Result<TypeNode, String> {
        let mut base = self.parse_primary_type()?;

        loop {
            if self.match_token(&[TokenType::LeftBracket]) {
                self.consume(TokenType::RightBracket, "Expected ']' after '[' in array type.")?;
                base = TypeNode::Array(Box::new(base));
            } else if self.match_token(&[TokenType::Question]) {
                base = TypeNode::Nullable(Box::new(base));
            } else {
                break;
            }
        }

        Ok(base)
    }

    fn parse_primary_type(&mut self) -> Result<TypeNode, String> {
        if self.match_token(&[TokenType::LeftParen]) {
            // Check for empty parameter list in function type: () => ReturnType
            if self.match_token(&[TokenType::RightParen]) {
                self.consume(TokenType::Arrow, "Expected '=>' after '()' in function type.")?;
                let return_type = self.parse_type()?;
                return Ok(TypeNode::Function {
                    params: Vec::new(),
                    return_type: Box::new(return_type),
                });
            }

            // Could be a grouped type (T) or function type (a: int, b: string) => R or (int, string) => R
            let mut items = Vec::new();
            let mut is_function = false;

            loop {
                // Check if parameter has a name like `a: int`
                if self.check_ident() && self.current + 1 < self.tokens.len() && self.tokens[self.current + 1].ty == TokenType::Colon {
                    let _param_name = self.consume_ident("Expected parameter name.")?;
                    self.consume(TokenType::Colon, "Expected ':' after parameter name.")?;
                    let param_ty = self.parse_type()?;
                    items.push(param_ty);
                    is_function = true;
                } else {
                    let ty = self.parse_type()?;
                    items.push(ty);
                }

                if !self.match_token(&[TokenType::Comma]) {
                    break;
                }
            }

            self.consume(TokenType::RightParen, "Expected ')' after type list.")?;

            if self.match_token(&[TokenType::Arrow]) {
                let return_type = self.parse_type()?;
                return Ok(TypeNode::Function {
                    params: items,
                    return_type: Box::new(return_type),
                });
            }

            if is_function {
                return Err(format!("Error at line {}: Expected '=>' after function parameter types.", self.peek().line));
            }

            if items.len() == 1 {
                return Ok(items.pop().unwrap());
            } else {
                // If multiple types in parens without '=>', treat as tuple
                return Ok(TypeNode::Tuple(items));
            }
        }

        // Tuple type: [A, B, C]
        if self.match_token(&[TokenType::LeftBracket]) {
            let mut elements = Vec::new();
            if !self.check(&TokenType::RightBracket) {
                loop {
                    elements.push(self.parse_type()?);
                    if !self.match_token(&[TokenType::Comma]) {
                        break;
                    }
                }
            }
            self.consume(TokenType::RightBracket, "Expected ']' after tuple types.")?;
            return Ok(TypeNode::Tuple(elements));
        }

        // Object shape type: { x: int, y?: string }
        if self.match_token(&[TokenType::LeftBrace]) {
            let mut props = Vec::new();
            if !self.check(&TokenType::RightBrace) {
                loop {
                    let name = self.consume_ident("Expected property name in object type.")?;
                    let optional = self.match_token(&[TokenType::Question]);
                    self.consume(TokenType::Colon, "Expected ':' after property name.")?;
                    let ty = self.parse_type()?;
                    props.push(PropertySignature { name, ty, optional });
                    if !self.match_token(&[TokenType::Comma]) && !self.match_token(&[TokenType::Case]) {
                        // Accept comma or newline/semicolon separation
                        if !self.check(&TokenType::RightBrace) && self.check_ident() {
                            continue;
                        }
                        break;
                    }
                }
            }
            self.consume(TokenType::RightBrace, "Expected '}' after object type.")?;
            return Ok(TypeNode::Object(props));
        }

        // Literal string type: "GET" | "POST"
        if let TokenType::String(ref s) = self.peek().ty.clone() {
            self.advance();
            return Ok(TypeNode::Literal(format!("\"{}\"", s)));
        }

        // Literal number type: 200 | 404
        if let TokenType::Number(n) = self.peek().ty {
            self.advance();
            return Ok(TypeNode::Literal(n.to_string()));
        }

        // Literal boolean type: true | false
        if self.match_token(&[TokenType::True]) {
            return Ok(TypeNode::Literal("true".to_string()));
        }
        if self.match_token(&[TokenType::False]) {
            return Ok(TypeNode::Literal("false".to_string()));
        }

        // Null type
        if self.match_token(&[TokenType::Null]) {
            return Ok(TypeNode::Primitive(PrimitiveType::Null));
        }

        // Identifier, Primitive, or Named<Generic>
        if self.check_ident() {
            let name = self.consume_ident("Expected type name.")?;

            // Check for generic type arguments: Name<T, U>
            if self.match_token(&[TokenType::Less]) {
                let mut type_args = Vec::new();
                if !self.check(&TokenType::Greater) {
                    loop {
                        type_args.push(self.parse_type()?);
                        if !self.match_token(&[TokenType::Comma]) {
                            break;
                        }
                    }
                }
                self.consume(TokenType::Greater, "Expected '>' after generic type arguments.")?;
                return Ok(TypeNode::Named(name, type_args));
            }

            return Ok(TypeNode::from_ident(&name));
        }

        Err(format!("Error at line {}: Unexpected token '{:?}' when parsing type.", self.peek().line, self.peek().ty))
    }
}
