use crate::protocol::{ToolCallContent, ToolCallResult, ToolCallTextContent, ToolDefinition};
use crate::tools::Tool;
use serde_json::{json, Value};
use std::future::Future;
use std::pin::Pin;

pub struct CalculatorTool;

impl CalculatorTool {
    pub fn new() -> Self {
        Self
    }
}

impl Default for CalculatorTool {
    fn default() -> Self {
        Self::new()
    }
}

impl Tool for CalculatorTool {
    fn name(&self) -> &str {
        "calculator"
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "calculator".to_string(),
            description: Some("Evaluate a mathematical expression. Supports +, -, *, /, and parentheses.".to_string()),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "expr": {
                        "type": "string",
                        "description": "The math expression to evaluate, e.g. '2 + 3 * (10 - 4) / 2'"
                    }
                },
                "required": ["expr"]
            }),
        }
    }

    fn call(&self, arguments: Option<Value>) -> Pin<Box<dyn Future<Output = anyhow::Result<ToolCallResult>> + Send + '_>> {
        Box::pin(async move {
            let expr = match arguments.and_then(|a| a.get("expr").and_then(|e| e.as_str()).map(|s| s.to_string())) {
                Some(e) => e,
                None => {
                    return Ok(ToolCallResult {
                        content: vec![ToolCallContent::Text(ToolCallTextContent {
                            text: "Error: Missing required argument 'expr'".to_string(),
                        })],
                        is_error: true,
                    });
                }
            };

            match evaluate(&expr) {
                Ok(result) => Ok(ToolCallResult {
                    content: vec![ToolCallContent::Text(ToolCallTextContent {
                        text: format!("{}", result),
                    })],
                    is_error: false,
                }),
                Err(err) => Ok(ToolCallResult {
                    content: vec![ToolCallContent::Text(ToolCallTextContent {
                        text: format!("Error: {}", err),
                    })],
                    is_error: true,
                }),
            }
        })
    }
}

// Simple recursive descent parser for mathematical expressions
#[derive(Debug, PartialEq, Clone)]
enum Token {
    Number(f64),
    Plus,
    Minus,
    Multiply,
    Divide,
    LParen,
    RParen,
}

fn tokenize(expr: &str) -> Result<Vec<Token>, String> {
    let mut tokens = Vec::new();
    let mut chars = expr.chars().peekable();

    while let Some(&c) = chars.peek() {
        match c {
            ' ' | '\t' | '\r' | '\n' => {
                chars.next();
            }
            '+' => {
                tokens.push(Token::Plus);
                chars.next();
            }
            '-' => {
                tokens.push(Token::Minus);
                chars.next();
            }
            '*' => {
                tokens.push(Token::Multiply);
                chars.next();
            }
            '/' => {
                tokens.push(Token::Divide);
                chars.next();
            }
            '(' => {
                tokens.push(Token::LParen);
                chars.next();
            }
            ')' => {
                tokens.push(Token::RParen);
                chars.next();
            }
            '0'..='9' | '.' => {
                let mut num_str = String::new();
                while let Some(&nc) = chars.peek() {
                    if nc.is_ascii_digit() || nc == '.' {
                        num_str.push(nc);
                        chars.next();
                    } else {
                        break;
                    }
                }
                let val: f64 = num_str.parse().map_err(|_| format!("Invalid number: {}", num_str))?;
                tokens.push(Token::Number(val));
            }
            _ => return Err(format!("Unexpected character: '{}'", c)),
        }
    }
    Ok(tokens)
}

struct Parser {
    tokens: Vec<Token>,
    pos: usize,
}

impl Parser {
    fn new(tokens: Vec<Token>) -> Self {
        Self { tokens, pos: 0 }
    }

    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.pos)
    }

    fn consume(&mut self) -> Option<Token> {
        if self.pos < self.tokens.len() {
            let t = self.tokens[self.pos].clone();
            self.pos += 1;
            Some(t)
        } else {
            None
        }
    }

    fn parse_expression(&mut self) -> Result<f64, String> {
        let mut value = self.parse_term()?;
        while let Some(tok) = self.peek().cloned() {
            match tok {
                Token::Plus => {
                    self.consume();
                    value += self.parse_term()?;
                }
                Token::Minus => {
                    self.consume();
                    value -= self.parse_term()?;
                }
                _ => break,
            }
        }
        Ok(value)
    }

    fn parse_term(&mut self) -> Result<f64, String> {
        let mut value = self.parse_factor()?;
        while let Some(tok) = self.peek().cloned() {
            match tok {
                Token::Multiply => {
                    self.consume();
                    value *= self.parse_factor()?;
                }
                Token::Divide => {
                    self.consume();
                    let denom = self.parse_factor()?;
                    if denom == 0.0 {
                        return Err("Division by zero".to_string());
                    }
                    value /= denom;
                }
                _ => break,
            }
        }
        Ok(value)
    }

    fn parse_factor(&mut self) -> Result<f64, String> {
        let tok = self.consume().ok_or_else(|| "Unexpected end of expression".to_string())?;
        match tok {
            Token::Number(val) => Ok(val),
            Token::Minus => {
                // Unary minus
                let factor = self.parse_factor()?;
                Ok(-factor)
            }
            Token::LParen => {
                let val = self.parse_expression()?;
                let next_tok = self.consume().ok_or_else(|| "Unmatched '('".to_string())?;
                if next_tok != Token::RParen {
                    return Err("Expected ')'".to_string());
                }
                Ok(val)
            }
            _ => Err(format!("Unexpected token: {:?}", tok)),
        }
    }
}

fn evaluate(expr: &str) -> Result<f64, String> {
    let tokens = tokenize(expr)?;
    if tokens.is_empty() {
        return Err("Empty expression".to_string());
    }
    let mut parser = Parser::new(tokens);
    let val = parser.parse_expression()?;
    if parser.pos < parser.tokens.len() {
        return Err("Extra tokens at end of expression".to_string());
    }
    Ok(val)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_calculator_evaluate() {
        assert_eq!(evaluate("2 + 3").unwrap(), 5.0);
        assert_eq!(evaluate("2 + 3 * 4").unwrap(), 14.0);
        assert_eq!(evaluate("(2 + 3) * 4").unwrap(), 20.0);
        assert_eq!(evaluate("10 / 2").unwrap(), 5.0);
        assert_eq!(evaluate("-5 + 3").unwrap(), -2.0);
        assert!(evaluate("1 / 0").is_err());
        assert!(evaluate("2 + (3").is_err());
    }
}

