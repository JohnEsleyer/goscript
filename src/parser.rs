use std::collections::HashSet;
use std::sync::{Mutex, OnceLock};

use crate::ast::{BinaryOp, Block, Expr, LogicalOp, Stmt, UnaryOp};
use crate::error::Error;
use crate::lexer::{Token, TokenKind};
use crate::value::Value;

pub struct Parser {
    tokens: Vec<Token>,
    pos: usize,
}

pub fn parse(tokens: Vec<Token>) -> Result<Vec<Stmt>, Error> {
    let mut parser = Parser { tokens, pos: 0 };
    parser.parse_program()
}

impl Parser {
    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.pos)
    }

    fn cur_kind(&self) -> Option<TokenKind> {
        self.peek().map(|t| t.kind.clone())
    }

    fn advance(&mut self) -> Result<Token, Error> {
        let tok = self
            .peek()
            .cloned()
            .ok_or_else(|| Error::runtime("unexpected end of token stream"))?;
        self.pos += 1;
        Ok(tok)
    }

    /// Line number of the current token (for source-mapped P1 instruction lines).
    fn cur_line(&self) -> usize {
        self.peek().map(|t| t.line).unwrap_or(1)
    }

    fn is_semicolon(&self) -> bool {
        matches!(self.cur_kind(), Some(TokenKind::Semicolon))
    }

    fn skip_semicolons(&mut self) {
        while self.is_semicolon() {
            self.pos += 1;
        }
    }

    fn expect(&mut self, kind: &TokenKind, what: &str) -> Result<Token, Error> {
        let tok = self.advance()?;
        if !kinds_equal(&tok.kind, kind) {
            return Err(Error::new(
                format!("expected {what}, found {}", tok.kind.describe()),
                tok.line,
                tok.col,
            ));
        }
        Ok(tok)
    }

    fn expect_identifier(&mut self, what: &str) -> Result<String, Error> {
        let tok = self.advance()?;
        match tok.kind {
            TokenKind::Identifier(name) => Ok(name),
            other => Err(Error::new(
                format!("expected {what}, found {}", other.describe()),
                tok.line,
                tok.col,
            )),
        }
    }

    fn skip_type_if_present(&mut self) {
        if matches!(self.cur_kind(), Some(TokenKind::Type(_))) {
            self.pos += 1;
        }
    }

    /// Skips an optional slice element type: either a built-in type token
    /// (`int`, `float64`, ...) or a user-defined identifier type (`Vector2`).
    fn skip_element_type_if_present(&mut self) {
        if matches!(
            self.cur_kind(),
            Some(TokenKind::Type(_)) | Some(TokenKind::Identifier(_))
        ) {
            self.pos += 1;
        }
    }

    fn is_rbrace_or_eof(&self) -> bool {
        matches!(self.cur_kind(), Some(TokenKind::RBrace) | Some(TokenKind::Eof))
    }

    fn parse_program(&mut self) -> Result<Block, Error> {
        let mut stmts = Vec::new();
        loop {
            self.skip_semicolons();
            match self.cur_kind() {
                Some(TokenKind::Eof) | None => break,
                _ => stmts.extend(self.parse_top_level_statement()?),
            }
        }
        Ok(stmts)
    }

    fn parse_block(&mut self) -> Result<Block, Error> {
        self.expect(&TokenKind::LBrace, "'{'")?;
        let mut stmts = Vec::new();
        loop {
            self.skip_semicolons();
            match self.cur_kind() {
                Some(TokenKind::RBrace) => {
                    self.pos += 1;
                    break;
                }
                Some(TokenKind::Eof) | None => {
                    let tok = self.peek().cloned().unwrap();
                    return Err(Error::new(
                        format!("expected '}}', found {}", tok.kind.describe()),
                        tok.line,
                        tok.col,
                    ));
                }
                _ => stmts.extend(self.parse_top_level_statement()?),
            }
        }
        Ok(stmts)
    }

    /// Parses one statement, except a grouped `import (...)` expands to many.
    fn parse_top_level_statement(&mut self) -> Result<Vec<Stmt>, Error> {
        if matches!(self.cur_kind(), Some(TokenKind::Import)) {
            self.parse_import()
        } else {
            Ok(vec![self.parse_statement()?])
        }
    }

    /// `import "path/file.gs"` or `import ("a.gs" "b.gs")`. Grouped imports
    /// return one `Stmt::Import` per path.
    fn parse_import(&mut self) -> Result<Vec<Stmt>, Error> {
        let line = self.cur_line();
        self.pos += 1; // 'import'
        let mut paths = Vec::new();
        if matches!(self.cur_kind(), Some(TokenKind::LParen)) {
            self.pos += 1;
            loop {
                self.skip_semicolons();
                match self.cur_kind() {
                    Some(TokenKind::RParen) => {
                        self.pos += 1;
                        break;
                    }
                    Some(TokenKind::Str(path)) => {
                        paths.push(path);
                        self.pos += 1;
                    }
                    Some(TokenKind::Eof) | None => {
                        let tok = self.peek().cloned().unwrap();
                        return Err(Error::new(
                            "unterminated grouped import, expected ')'",
                            tok.line,
                            tok.col,
                        ));
                    }
                    _ => {
                        let tok = self.peek().cloned().unwrap();
                        return Err(Error::new(
                            "expected import path string",
                            tok.line,
                            tok.col,
                        ));
                    }
                }
            }
        } else {
            let tok = self.advance()?;
            match tok.kind {
                TokenKind::Str(path) => paths.push(path),
                other => {
                    return Err(Error::new(
                        format!("expected import path string, found {}", other.describe()),
                        tok.line,
                        tok.col,
                    ));
                }
            }
        }
        Ok(paths
            .into_iter()
            .map(|path| Stmt::Import { path, line })
            .collect())
    }

    fn parse_statement(&mut self) -> Result<Stmt, Error> {
        match self.cur_kind() {
            Some(TokenKind::Var) => self.parse_var_decl(),
            Some(TokenKind::KwType) => self.parse_struct_decl(),
            Some(TokenKind::Func) => self.parse_func_decl(),
            Some(TokenKind::Import) => {
                let mut imports = self.parse_import()?;
                if imports.len() > 1 {
                    let tok = self.peek().cloned().unwrap();
                    return Err(Error::new(
                        "grouped imports are only allowed at file scope",
                        tok.line,
                        tok.col,
                    ));
                }
                Ok(imports.remove(0))
            }
            Some(TokenKind::If) => self.parse_if(),
            Some(TokenKind::For) => self.parse_for(),
            Some(TokenKind::Switch) => self.parse_switch(),
            Some(TokenKind::Return) => {
                let line = self.cur_line();
                self.pos += 1;
                let stmt = if self.is_semicolon() || self.is_rbrace_or_eof() {
                    Stmt::Return(None, line)
                } else {
                    Stmt::Return(Some(self.parse_expression()?), line)
                };
                Ok(stmt)
            }
            Some(TokenKind::Break) => {
                let line = self.cur_line();
                self.pos += 1;
                Ok(Stmt::Break(line))
            }
            Some(TokenKind::Continue) => {
                let line = self.cur_line();
                self.pos += 1;
                Ok(Stmt::Continue(line))
            }
            Some(TokenKind::LBrace) => {
                let tok = self.peek().cloned().unwrap();
                Err(Error::new("unexpected '{'", tok.line, tok.col))
            }
            _ => self.parse_expr_statement(),
        }
    }

    fn parse_var_decl(&mut self) -> Result<Stmt, Error> {
        let line = self.cur_line();
        self.pos += 1; // 'var'
        let name = self.expect_identifier("variable name")?;
        self.skip_type_if_present();
        let init = match self.cur_kind() {
            Some(TokenKind::Assign) | Some(TokenKind::ShortAssign) => {
                self.pos += 1;
                Some(self.parse_expression()?)
            }
            _ => None,
        };
        Ok(Stmt::VarDecl { name, init, line })
    }

    fn parse_struct_decl(&mut self) -> Result<Stmt, Error> {
        let line = self.cur_line();
        self.pos += 1; // 'type'
        let name = self.expect_identifier("struct type name")?;
        self.expect(&TokenKind::Struct, "'struct'")?;
        self.expect(&TokenKind::LBrace, "'{'")?;
        let mut fields = Vec::new();
        loop {
            self.skip_semicolons();
            match self.cur_kind() {
                Some(TokenKind::RBrace) | Some(TokenKind::Eof) | None => break,
                _ => {
                    let field_name = self.expect_identifier("field name")?;
                    self.skip_type_if_present();
                    fields.push(field_name);
                }
            }
        }
        self.expect(&TokenKind::RBrace, "'}'")?;
        Ok(Stmt::StructDecl { name, fields, line })
    }

    fn parse_func_decl(&mut self) -> Result<Stmt, Error> {
        let line = self.cur_line();
        self.pos += 1; // 'func'
        let mut receiver = None;
        // Receiver method: `func (p *Player) TakeDamage(amount int)`.
        if matches!(self.cur_kind(), Some(TokenKind::LParen)) {
            self.pos += 1;
            let recv_name = self.expect_identifier("receiver name")?;
            if matches!(self.cur_kind(), Some(TokenKind::Star)) {
                self.pos += 1;
            }
            let recv_type = self.expect_identifier("receiver type")?;
            self.expect(&TokenKind::RParen, "')'")?;
            receiver = Some((recv_name, recv_type));
        }
        let name = self.expect_identifier("function name")?;
        self.expect(&TokenKind::LParen, "'('")?;
        let mut params = Vec::new();
        loop {
            match self.cur_kind() {
                Some(TokenKind::RParen) | Some(TokenKind::Eof) => break,
                Some(TokenKind::Type(_)) => {
                    let tok = self.advance()?;
                    return Err(Error::new(
                        format!("expected parameter name, found {}", tok.kind.describe()),
                        tok.line,
                        tok.col,
                    ));
                }
                _ => {
                    let param = self.expect_identifier("parameter name")?;
                    self.skip_type_if_present();
                    params.push(param);
                    if matches!(self.cur_kind(), Some(TokenKind::Comma)) {
                        self.pos += 1;
                    } else {
                        break;
                    }
                }
            }
        }
        self.expect(&TokenKind::RParen, "')'")?;
        self.skip_type_if_present();
        let body = self.parse_block()?;
        Ok(Stmt::FuncDecl {
            name,
            receiver,
            params,
            body,
            line,
        })
    }

    fn parse_if(&mut self) -> Result<Stmt, Error> {
        let line = self.cur_line();
        self.pos += 1; // 'if'
        let condition = self.parse_expression()?;
        let then_branch = self.parse_block()?;
        let else_branch = if matches!(self.cur_kind(), Some(TokenKind::Else)) {
            self.pos += 1;
            if matches!(self.cur_kind(), Some(TokenKind::If)) {
                Some(vec![self.parse_if()?])
            } else {
                Some(self.parse_block()?)
            }
        } else {
            None
        };
        Ok(Stmt::If {
            condition,
            then_branch,
            else_branch,
            line,
        })
    }

    fn parse_for(&mut self) -> Result<Stmt, Error> {
        let line = self.cur_line();
        self.pos += 1; // 'for'
        if matches!(self.cur_kind(), Some(TokenKind::LBrace)) {
            let body = self.parse_block()?;
            return Ok(Stmt::For {
                init: None,
                condition: None,
                post: None,
                body,
                line,
            });
        }

        // `for i, v := range coll { ... }` / `for i := range coll { ... }`.
        if self.is_range_clause() {
            let (index_var, value_var, target) = self.parse_range_clause()?;
            let body = self.parse_block()?;
            // Desugar into an equivalent indexed for loop over the target:
            //   init: var i = 0
            //   cond: i < len(target)
            //   post: i = i + 1
            //   body: var v = target[i] (when a value variable is requested)
            let init = Stmt::VarDecl {
                name: index_var.clone(),
                init: Some(Expr::Literal(Value::Int(0))),
                line,
            };
            let condition = Some(Expr::Binary {
                left: Box::new(Expr::Identifier(index_var.clone())),
                op: BinaryOp::Less,
                right: Box::new(Expr::Call {
                    callee: "len".to_string(),
                    args: vec![target.clone()],
                }),
            });
            let post = Stmt::Assign {
                name: index_var.clone(),
                value: Expr::Binary {
                    left: Box::new(Expr::Identifier(index_var.clone())),
                    op: BinaryOp::Add,
                    right: Box::new(Expr::Literal(Value::Int(1))),
                },
                line,
            };
            let mut range_body = Vec::new();
            if let Some(value_var) = value_var {
                range_body.push(Stmt::VarDecl {
                    name: value_var,
                    init: Some(Expr::GetIndex {
                        object: Box::new(target.clone()),
                        index: Box::new(Expr::Identifier(index_var)),
                    }),
                    line,
                });
            }
            range_body.extend(body);
            return Ok(Stmt::For {
                init: Some(Box::new(init)),
                condition,
                post: Some(Box::new(post)),
                body: range_body,
                line,
            });
        }

        let init_clause = self.parse_for_clause()?;
        if self.is_semicolon() {
            self.pos += 1;
            let condition = if self.is_semicolon()
                || matches!(self.cur_kind(), Some(TokenKind::LBrace))
            {
                None
            } else {
                Some(self.parse_expression()?)
            };
            let post = if self.is_semicolon() {
                self.pos += 1;
                Some(Box::new(self.parse_for_clause()?))
            } else {
                None
            };
            let body = self.parse_block()?;
            Ok(Stmt::For {
                init: Some(Box::new(init_clause)),
                condition,
                post,
                body,
                line,
            })
        } else {
            let condition = match init_clause {
                Stmt::Expr(expr, _) => Some(expr),
                _ => {
                    let tok = self.peek().cloned().unwrap();
                    return Err(Error::new(
                        "invalid for-loop clause, expected condition or 'init; cond; post'",
                        tok.line,
                        tok.col,
                    ));
                }
            };
            let body = self.parse_block()?;
            Ok(Stmt::For {
                init: None,
                condition,
                post: None,
                body,
                line,
            })
        }
    }

    /// `for <id> [, <id>] := range <target>`?
    fn is_range_clause(&self) -> bool {
        let is_id = |k: Option<TokenKind>| matches!(k, Some(TokenKind::Identifier(_)));
        let is_range = |k: Option<TokenKind>| {
            matches!(k, Some(TokenKind::Identifier(word)) if word == "range")
        };
        is_id(self.tokens.get(self.pos).map(|t| t.kind.clone()))
            && (self.tokens.get(self.pos + 1).map(|t| t.kind.clone())
                == Some(TokenKind::ShortAssign))
            && is_range(self.tokens.get(self.pos + 2).map(|t| t.kind.clone()))
            || (is_id(self.tokens.get(self.pos).map(|t| t.kind.clone()))
                && self.tokens.get(self.pos + 1).map(|t| t.kind.clone())
                    == Some(TokenKind::Comma)
                && is_id(self.tokens.get(self.pos + 2).map(|t| t.kind.clone()))
                && self.tokens.get(self.pos + 3).map(|t| t.kind.clone())
                    == Some(TokenKind::ShortAssign)
                && is_range(self.tokens.get(self.pos + 4).map(|t| t.kind.clone())))
    }

    /// Consumes `i [, v] := range` and returns `(index, value, target expr)`.
    fn parse_range_clause(&mut self) -> Result<(String, Option<String>, Expr), Error> {
        let index_var = self.expect_identifier("range index variable")?;
        let value_var = if matches!(self.cur_kind(), Some(TokenKind::Comma)) {
            self.pos += 1;
            Some(self.expect_identifier("range value variable")?)
        } else {
            None
        };
        self.expect(&TokenKind::ShortAssign, "':='")?;
        self.expect_identifier("'range' keyword")?;
        let target = self.parse_expression()?;
        Ok((index_var, value_var, target))
    }

    fn parse_for_clause(&mut self) -> Result<Stmt, Error> {
        if matches!(self.cur_kind(), Some(TokenKind::Var)) {
            return self.parse_var_decl();
        }
        self.parse_expr_statement()
    }

    fn parse_switch(&mut self) -> Result<Stmt, Error> {
        let line = self.cur_line();
        self.pos += 1; // 'switch'
        let expr = self.parse_expression()?;
        self.expect(&TokenKind::LBrace, "'{'")?;
        let mut cases = Vec::new();
        let mut default_case = None;
        loop {
            match self.cur_kind() {
                Some(TokenKind::Case) => {
                    self.pos += 1;
                    let case_expr = self.parse_expression()?;
                    self.expect(&TokenKind::Colon, "':'")?;
                    let mut body = Vec::new();
                    loop {
                        self.skip_semicolons();
                        if matches!(
                            self.cur_kind(),
                            Some(TokenKind::Case)
                                | Some(TokenKind::Default)
                                | Some(TokenKind::RBrace)
                                | Some(TokenKind::Eof)
                                | None
                        ) {
                            break;
                        }
                        body.push(self.parse_statement()?);
                    }
                    cases.push((case_expr, body));
                }
                Some(TokenKind::Default) => {
                    self.pos += 1;
                    self.expect(&TokenKind::Colon, "':'")?;
                    let mut body = Vec::new();
                    loop {
                        self.skip_semicolons();
                        if matches!(
                            self.cur_kind(),
                            Some(TokenKind::Case)
                                | Some(TokenKind::Default)
                                | Some(TokenKind::RBrace)
                                | Some(TokenKind::Eof)
                                | None
                        ) {
                            break;
                        }
                        body.push(self.parse_statement()?);
                    }
                    default_case = Some(body);
                }
                Some(TokenKind::RBrace) => {
                    self.pos += 1;
                    break;
                }
                Some(TokenKind::Eof) | None => {
                    let tok = self.peek().cloned().unwrap();
                    return Err(Error::new(
                        format!(
                            "expected 'case', 'default' or '}}', found {}",
                            tok.kind.describe()
                        ),
                        tok.line,
                        tok.col,
                    ));
                }
                _ => {
                    let tok = self.peek().cloned().unwrap();
                    return Err(Error::new(
                        format!(
                            "expected 'case' or 'default', found {}",
                            tok.kind.describe()
                        ),
                        tok.line,
                        tok.col,
                    ));
                }
            }
        }
        Ok(Stmt::Switch {
            expr,
            cases,
            default_case,
            line,
        })
    }

    fn parse_expr_statement(&mut self) -> Result<Stmt, Error> {
        let line = self.cur_line();
        let expr = self.parse_expression()?;
        match self.cur_kind() {
            Some(TokenKind::Assign) => {
                self.pos += 1;
                let value = self.parse_expression()?;
                match expr {
                    Expr::Identifier(name) => Ok(Stmt::Assign { name, value, line }),
                    Expr::GetField { object, field } => Ok(Stmt::SetField {
                        object: *object,
                        field,
                        value,
                        line,
                    }),
                    Expr::GetIndex { object, index } => Ok(Stmt::SetIndex {
                        object: *object,
                        index: *index,
                        value,
                        line,
                    }),
                    _ => {
                        let tok = self.peek().cloned().unwrap();
                        Err(Error::new("invalid assignment target", tok.line, tok.col))
                    }
                }
            }
            Some(TokenKind::ShortAssign) => {
                self.pos += 1;
                let value = self.parse_expression()?;
                match expr {
                    Expr::Identifier(name) => Ok(Stmt::VarDecl {
                        name,
                        init: Some(value),
                        line,
                    }),
                    _ => {
                        let tok = self.peek().cloned().unwrap();
                        Err(Error::new(
                            "invalid short assignment target",
                            tok.line,
                            tok.col,
                        ))
                    }
                }
            }
            // Compound assignment: `a += expr` desugars to `a = a + expr`.
            Some(TokenKind::PlusAssign)
            | Some(TokenKind::MinusAssign)
            | Some(TokenKind::StarAssign)
            | Some(TokenKind::SlashAssign) => {
                let op = match self.cur_kind().unwrap() {
                    TokenKind::PlusAssign => BinaryOp::Add,
                    TokenKind::MinusAssign => BinaryOp::Sub,
                    TokenKind::StarAssign => BinaryOp::Mul,
                    _ => BinaryOp::Div,
                };
                self.pos += 1;
                let value = self.parse_expression()?;
                self.desugar_compound(expr, op, value, line)
            }
            // `x++` / `x--` are sugar for `x = x + 1` / `x = x - 1`.
            Some(TokenKind::PlusPlus) | Some(TokenKind::MinusMinus) => {
                let op = if matches!(self.cur_kind(), Some(TokenKind::PlusPlus)) {
                    BinaryOp::Add
                } else {
                    BinaryOp::Sub
                };
                self.pos += 1;
                self.desugar_compound(expr, op, Expr::Literal(Value::Int(1)), line)
            }
            _ => Ok(Stmt::Expr(expr, line)),
        }
    }

    /// Builds a `lhs op= rhs` statement from a parsed target expression and a
    /// desugared binary expression, dispatching on the target kind.
    fn desugar_compound(
        &self,
        expr: Expr,
        op: BinaryOp,
        rhs: Expr,
        line: usize,
    ) -> Result<Stmt, Error> {
        match &expr {
            Expr::Identifier(name) => Ok(Stmt::Assign {
                name: name.clone(),
                value: Expr::Binary {
                    left: Box::new(Expr::Identifier(name.clone())),
                    op,
                    right: Box::new(rhs),
                },
                line,
            }),
            Expr::GetField { object, field } => Ok(Stmt::SetField {
                object: (**object).clone(),
                field: field.clone(),
                value: Expr::Binary {
                    left: Box::new(Expr::GetField {
                        object: object.clone(),
                        field: field.clone(),
                    }),
                    op,
                    right: Box::new(rhs),
                },
                line,
            }),
            Expr::GetIndex { object, index } => Ok(Stmt::SetIndex {
                object: (**object).clone(),
                index: (**index).clone(),
                value: Expr::Binary {
                    left: Box::new(Expr::GetIndex {
                        object: object.clone(),
                        index: index.clone(),
                    }),
                    op,
                    right: Box::new(rhs),
                },
                line,
            }),
            _ => {
                let tok = self.peek().cloned().unwrap();
                Err(Error::new(
                    "invalid compound assignment target",
                    tok.line,
                    tok.col,
                ))
            }
        }
    }

    fn parse_expression(&mut self) -> Result<Expr, Error> {
        self.parse_or()
    }

    fn parse_or(&mut self) -> Result<Expr, Error> {
        let mut left = self.parse_and()?;
        while matches!(self.cur_kind(), Some(TokenKind::Or)) {
            self.pos += 1;
            let right = self.parse_and()?;
            left = Expr::Logical {
                left: Box::new(left),
                op: LogicalOp::Or,
                right: Box::new(right),
            };
        }
        Ok(left)
    }

    fn parse_and(&mut self) -> Result<Expr, Error> {
        let mut left = self.parse_equality()?;
        while matches!(self.cur_kind(), Some(TokenKind::And)) {
            self.pos += 1;
            let right = self.parse_equality()?;
            left = Expr::Logical {
                left: Box::new(left),
                op: LogicalOp::And,
                right: Box::new(right),
            };
        }
        Ok(left)
    }

    fn parse_equality(&mut self) -> Result<Expr, Error> {
        let mut left = self.parse_comparison()?;
        loop {
            let op = match self.cur_kind() {
                Some(TokenKind::Equal) => BinaryOp::Equal,
                Some(TokenKind::NotEqual) => BinaryOp::NotEqual,
                _ => break,
            };
            self.pos += 1;
            let right = self.parse_comparison()?;
            left = Expr::Binary {
                left: Box::new(left),
                op,
                right: Box::new(right),
            };
        }
        Ok(left)
    }

    fn parse_comparison(&mut self) -> Result<Expr, Error> {
        let mut left = self.parse_term()?;
        loop {
            let op = match self.cur_kind() {
                Some(TokenKind::Less) => BinaryOp::Less,
                Some(TokenKind::LessEqual) => BinaryOp::LessEqual,
                Some(TokenKind::Greater) => BinaryOp::Greater,
                Some(TokenKind::GreaterEqual) => BinaryOp::GreaterEqual,
                _ => break,
            };
            self.pos += 1;
            let right = self.parse_term()?;
            left = Expr::Binary {
                left: Box::new(left),
                op,
                right: Box::new(right),
            };
        }
        Ok(left)
    }

    fn parse_term(&mut self) -> Result<Expr, Error> {
        let mut left = self.parse_factor()?;
        loop {
            let op = match self.cur_kind() {
                Some(TokenKind::Plus) => BinaryOp::Add,
                Some(TokenKind::Minus) => BinaryOp::Sub,
                _ => break,
            };
            self.pos += 1;
            let right = self.parse_factor()?;
            left = Expr::Binary {
                left: Box::new(left),
                op,
                right: Box::new(right),
            };
        }
        Ok(left)
    }

    fn parse_factor(&mut self) -> Result<Expr, Error> {
        let mut left = self.parse_unary()?;
        loop {
            let op = match self.cur_kind() {
                Some(TokenKind::Star) => BinaryOp::Mul,
                Some(TokenKind::Slash) => BinaryOp::Div,
                Some(TokenKind::Percent) => BinaryOp::Rem,
                _ => break,
            };
            self.pos += 1;
            let right = self.parse_unary()?;
            left = Expr::Binary {
                left: Box::new(left),
                op,
                right: Box::new(right),
            };
        }
        Ok(left)
    }

    fn parse_unary(&mut self) -> Result<Expr, Error> {
        match self.cur_kind() {
            Some(TokenKind::Minus) => {
                self.pos += 1;
                let inner = self.parse_unary()?;
                Ok(Expr::Unary {
                    op: UnaryOp::Negate,
                    expr: Box::new(inner),
                })
            }
            Some(TokenKind::Not) => {
                self.pos += 1;
                let inner = self.parse_unary()?;
                Ok(Expr::Unary {
                    op: UnaryOp::Not,
                    expr: Box::new(inner),
                })
            }
            _ => self.parse_postfix(),
        }
    }

    fn parse_postfix(&mut self) -> Result<Expr, Error> {
        let mut expr = self.parse_primary()?;
        loop {
            if matches!(self.cur_kind(), Some(TokenKind::Dot)) {
                // Package name resolution: `math.Sqrt(4)`, `rand.Float()`,
                // `fmt.Println(...)`, host packages like `groot.Log(...)`, and
                // bare package globals like `math.Pi`.
                if let Expr::Identifier(base) = &expr {
                    if is_package(base.as_str()) {
                        self.pos += 1;
                        let symbol = self.expect_identifier("package member")?;
                        let dotted = format!("{base}.{symbol}");
                        if matches!(self.cur_kind(), Some(TokenKind::LParen)) {
                            self.pos += 1;
                            let mut args = Vec::new();
                            loop {
                                if matches!(
                                    self.cur_kind(),
                                    Some(TokenKind::RParen) | Some(TokenKind::Eof)
                                ) {
                                    break;
                                }
                                args.push(self.parse_expression()?);
                                if matches!(self.cur_kind(), Some(TokenKind::Comma)) {
                                    self.pos += 1;
                                } else {
                                    break;
                                }
                            }
                            self.expect(&TokenKind::RParen, "')'")?;
                            expr = Expr::Call { callee: dotted, args };
                        } else {
                            expr = Expr::Identifier(dotted);
                        }
                        continue;
                    }
                }
                self.pos += 1;
                let field = self.expect_identifier("field name")?;
                if matches!(self.cur_kind(), Some(TokenKind::LParen)) {
                    self.pos += 1;
                    let mut args = Vec::new();
                    loop {
                        if matches!(
                            self.cur_kind(),
                            Some(TokenKind::RParen) | Some(TokenKind::Eof)
                        ) {
                            break;
                        }
                        args.push(self.parse_expression()?);
                        if matches!(self.cur_kind(), Some(TokenKind::Comma)) {
                            self.pos += 1;
                        } else {
                            break;
                        }
                    }
                    self.expect(&TokenKind::RParen, "')'")?;
                    expr = Expr::MethodCall {
                        receiver: Box::new(expr),
                        method: field,
                        args,
                    };
                } else {
                    expr = Expr::GetField {
                        object: Box::new(expr),
                        field,
                    };
                }
            } else if matches!(self.cur_kind(), Some(TokenKind::LBracket)) {
                self.pos += 1;
                let index = self.parse_expression()?;
                self.expect(&TokenKind::RBracket, "']'")?;
                expr = Expr::GetIndex {
                    object: Box::new(expr),
                    index: Box::new(index),
                };
            } else {
                break;
            }
        }
        Ok(expr)
    }

    fn parse_primary(&mut self) -> Result<Expr, Error> {
        let tok = self.advance()?;
        match tok.kind {
            TokenKind::Int(v) => Ok(Expr::Literal(Value::Int(v))),
            TokenKind::Float(v) => Ok(Expr::Literal(Value::Float(v))),
            TokenKind::Str(s) => Ok(Expr::Literal(Value::String(s))),
            TokenKind::True => Ok(Expr::Literal(Value::Bool(true))),
            TokenKind::False => Ok(Expr::Literal(Value::Bool(false))),
            TokenKind::Nil => Ok(Expr::Literal(Value::Nil)),
            TokenKind::LBracket => {
                // Slice literal: `[]T{...}`. The element type is optional
                // sugar right after the brackets.
                self.expect(&TokenKind::RBracket, "']'")?;
                self.skip_element_type_if_present();
                self.expect(&TokenKind::LBrace, "'{'")?;
                let mut items = Vec::new();
                loop {
                    match self.cur_kind() {
                        Some(TokenKind::RBrace) => {
                            self.pos += 1;
                            break;
                        }
                        Some(TokenKind::Eof) | None => break,
                        _ => {
                            items.push(self.parse_expression()?);
                            if matches!(self.cur_kind(), Some(TokenKind::Comma)) {
                                self.pos += 1;
                            }
                        }
                    }
                }
                Ok(Expr::SliceInit { items })
            }
            TokenKind::Identifier(name) => {
                match name.as_str() {
                    // Map literal: `map[K]V{ "key": value, ... }`.
                    "map" if matches!(self.cur_kind(), Some(TokenKind::LBracket)) => {
                        self.pos += 1; // '['
                        self.skip_type_if_present(); // key type
                        self.expect(&TokenKind::RBracket, "']'")?;
                        self.skip_type_if_present(); // value type
                        self.expect(&TokenKind::LBrace, "'{'")?;
                        let mut entries = Vec::new();
                        loop {
                            match self.cur_kind() {
                                Some(TokenKind::RBrace) => {
                                    self.pos += 1;
                                    break;
                                }
                                Some(TokenKind::Eof) | None => break,
                                _ => {
                                    let key = self.parse_expression()?;
                                    self.expect(&TokenKind::Colon, "':' after map key")?;
                                    let value = self.parse_expression()?;
                                    entries.push((key, value));
                                    if matches!(self.cur_kind(), Some(TokenKind::Comma)) {
                                        self.pos += 1;
                                    }
                                }
                            }
                        }
                        Ok(Expr::MapInit { entries })
                    }
                    name if matches!(self.cur_kind(), Some(TokenKind::LBrace))
                        && is_struct_literal_ahead(&self.tokens, self.pos - 1) =>
                    {
                        self.parse_struct_init(name.to_string(), tok.line, tok.col)
                    }
                    name if matches!(self.cur_kind(), Some(TokenKind::LParen)) => {
                        self.pos += 1;
                        let mut args = Vec::new();
                        loop {
                            if matches!(
                                self.cur_kind(),
                                Some(TokenKind::RParen) | Some(TokenKind::Eof)
                            ) {
                                break;
                            }
                            args.push(self.parse_expression()?);
                            if matches!(self.cur_kind(), Some(TokenKind::Comma)) {
                                self.pos += 1;
                            } else {
                                break;
                            }
                        }
                        self.expect(&TokenKind::RParen, "')'")?;
                        Ok(Expr::Call {
                            callee: name.to_string(),
                            args,
                        })
                    }
                    _ => Ok(Expr::Identifier(name)),
                }
            }
            TokenKind::LParen => {
                let inner = self.parse_expression()?;
                self.expect(&TokenKind::RParen, "')'")?;
                Ok(inner)
            }
            TokenKind::Type(name) => {
                // Explicit type casts are written as plain calls on a type
                // name: `int(x)`, `float64(x)`, `string(v)`, `bool(flag)`.
                if matches!(self.cur_kind(), Some(TokenKind::LParen)) {
                    self.pos += 1;
                    let mut args = Vec::new();
                    loop {
                        if matches!(
                            self.cur_kind(),
                            Some(TokenKind::RParen) | Some(TokenKind::Eof)
                        ) {
                            break;
                        }
                        args.push(self.parse_expression()?);
                        if matches!(self.cur_kind(), Some(TokenKind::Comma)) {
                            self.pos += 1;
                        } else {
                            break;
                        }
                    }
                    self.expect(&TokenKind::RParen, "')'")?;
                    Ok(Expr::Call {
                        callee: name,
                        args,
                    })
                } else {
                    Err(Error::new(
                        format!("unexpected type '{}' in expression", name),
                        tok.line,
                        tok.col,
                    ))
                }
            }
            other => Err(Error::new(
                format!("unexpected {}, expected expression", other.describe()),
                tok.line,
                tok.col,
            )),
        }
    }

    fn parse_struct_init(
        &mut self,
        name: String,
        _line: usize,
        _col: usize,
    ) -> Result<Expr, Error> {
        self.pos += 1; // '{'
        let mut fields = Vec::new();
        loop {
            match self.cur_kind() {
                Some(TokenKind::RBrace) | Some(TokenKind::Eof) => break,
                _ => {
                    let field_name = self.expect_identifier("field name")?;
                    self.expect(&TokenKind::Colon, "':' after field name")?;
                    let value = self.parse_expression()?;
                    fields.push((field_name, value));
                    if matches!(self.cur_kind(), Some(TokenKind::Comma)) {
                        self.pos += 1;
                    } else {
                        break;
                    }
                }
            }
        }
        self.expect(&TokenKind::RBrace, "'}'")?;
        Ok(Expr::StructInit { name, fields })
    }
}

/// Process-wide registry of script-visible package prefixes. Seeded with the
/// native standard library names; engine hosts declare their own packages
/// (`declare_package`) as they register dotted native functions such as
/// `"groot.GetAxis"`. Shared across VMs, matching the engine-focused design.
static PACKAGES: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();

fn packages() -> &'static Mutex<HashSet<String>> {
    PACKAGES.get_or_init(|| {
        let mut set = HashSet::new();
        for name in ["math", "fmt", "rand", "time"] {
            set.insert(name.to_string());
        }
        Mutex::new(set)
    })
}

/// Declare a package prefix (e.g. `"groot"`) so that dotted member access like
/// `groot.GetAxis(...)` compiles to a dotted call instead of a struct field.
pub fn declare_package(name: &str) {
    packages().lock().unwrap().insert(name.to_string());
}

fn is_package(name: &str) -> bool {
    packages().lock().unwrap().contains(name)
}

fn kinds_equal(a: &TokenKind, b: &TokenKind) -> bool {
    match (a, b) {
        (TokenKind::Identifier(_), TokenKind::Identifier(_)) => true,
        (TokenKind::Type(_), TokenKind::Type(_)) => true,
        (TokenKind::Int(_), TokenKind::Int(_)) => true,
        (TokenKind::Float(_), TokenKind::Float(_)) => true,
        (TokenKind::Str(_), TokenKind::Str(_)) => true,
        (a, b) => a == b,
    }
}

fn is_struct_literal_ahead(tokens: &[Token], pos: usize) -> bool {
    match (tokens.get(pos + 1), tokens.get(pos + 2), tokens.get(pos + 3)) {
        (Some(a), Some(b), Some(c)) if a.kind == TokenKind::LBrace => match &b.kind {
            TokenKind::Identifier(_) => c.kind == TokenKind::Colon,
            _ => false,
        },
        (Some(a), Some(b), _) if a.kind == TokenKind::LBrace && b.kind == TokenKind::RBrace => true,
        _ => false,
    }
}